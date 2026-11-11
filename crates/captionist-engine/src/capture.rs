//! Audio capture through PipeWire's `pw-record` (part of every PipeWire
//! install), as 16 kHz mono f32.
//!
//! Sources: speakers (monitor of the default output), microphone, both mixed,
//! or a single app's own output stream.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Speakers,
    Microphone,
    Both,
    /// One app's audio, by its PipeWire `application.name` (e.g. "Firefox").
    App(String),
}

impl Source {
    pub fn parse(kind: &str, app: &str) -> Self {
        match kind {
            "mic" | "microphone" => Source::Microphone,
            "both" => Source::Both,
            "app" if !app.is_empty() => Source::App(app.to_string()),
            _ => Source::Speakers,
        }
    }
}

/// An app currently playing audio.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AudioApp {
    /// PipeWire `application.name`, used to find the app again later.
    pub name: String,
    /// What it's playing (`media.name`), e.g. a tab title.
    pub media: String,
    pub icon: Option<String>,
}

/// Apps with an audio output stream right now (one entry per app).
pub fn list_audio_apps() -> Vec<AudioApp> {
    let mut apps: Vec<AudioApp> = vec![];
    for (_, props) in output_streams() {
        let Some(name) = props.get("application.name").and_then(|v| v.as_str()) else {
            continue;
        };
        if name.is_empty() || name == "pw-record" || apps.iter().any(|a| a.name == name) {
            continue;
        }
        apps.push(AudioApp {
            name: name.to_string(),
            media: props
                .get("media.name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            icon: props
                .get("application.icon-name")
                .and_then(|v| v.as_str())
                .map(str::to_string),
        });
    }
    apps
}

/// (object.serial, props) of every audio output stream.
fn output_streams() -> Vec<(u64, serde_json::Map<String, serde_json::Value>)> {
    let Ok(out) = Command::new("pw-dump").stderr(Stdio::null()).output() else {
        return vec![];
    };
    let Ok(objects) = serde_json::from_slice::<Vec<serde_json::Value>>(&out.stdout) else {
        return vec![];
    };
    objects
        .into_iter()
        .filter_map(|o| {
            let props = o.get("info")?.get("props")?.as_object()?.clone();
            if props.get("media.class")?.as_str()? != "Stream/Output/Audio" {
                return None;
            }
            let serial = props.get("object.serial")?.as_u64()?;
            Some((serial, props))
        })
        .collect()
}

fn app_stream_serials(app: &str) -> Vec<u64> {
    output_streams()
        .into_iter()
        .filter(|(_, p)| p.get("application.name").and_then(|v| v.as_str()) == Some(app))
        .map(|(s, _)| s)
        .collect()
}

/// 50 ms of audio at 16 kHz.
const READ_SAMPLES: usize = 800;

enum Target {
    Speakers,
    Microphone,
    Stream(u64),
}

fn spawn(target: &Target) -> Result<Child> {
    let mut cmd = Command::new("pw-record");
    let extra = match target {
        Target::Speakers => "stream.capture.sink=true",
        _ => "",
    };
    cmd.args(["-P", &format!("{{ node.name=captionist-capture node.description=\"Captionist live captions\" {extra} }}")]);
    if let Target::Stream(serial) = target {
        cmd.args(["--target", &serial.to_string()]);
    }
    cmd.args([
        "--rate",
        "16000",
        "--channels",
        "1",
        "--format",
        "f32",
        "--raw",
        "--latency",
        "50ms",
        "-",
    ]);
    cmd.stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null());
    cmd.spawn()
        .context("could not start pw-record (is PipeWire installed?)")
}

fn reader(mut child_out: impl Read + Send + 'static, idx: usize, tx: Sender<(usize, Vec<f32>)>) {
    thread::spawn(move || {
        let mut buf = vec![0u8; READ_SAMPLES * 4];
        loop {
            if child_out.read_exact(&mut buf).is_err() {
                break;
            }
            let samples = buf
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .collect();
            if tx.send((idx, samples)).is_err() {
                break;
            }
        }
    });
}

pub struct Capture {
    children: Arc<Mutex<Vec<Child>>>,
    stop: Arc<AtomicBool>,
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        for c in self.children.lock().unwrap().iter_mut() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Capture {
    /// Starts capturing; audio arrives on the receiver in 50 ms pieces.
    pub fn start(source: Source) -> Result<(Capture, Receiver<Vec<f32>>)> {
        let children = Arc::new(Mutex::new(vec![]));
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();

        if let Source::App(app) = source {
            Self::follow_app(app, children.clone(), stop.clone(), tx);
            return Ok((Capture { children, stop }, rx));
        }

        let targets: Vec<Target> = match source {
            Source::Speakers => vec![Target::Speakers],
            Source::Microphone => vec![Target::Microphone],
            _ => vec![Target::Speakers, Target::Microphone],
        };
        let (raw_tx, raw_rx) = mpsc::channel::<(usize, Vec<f32>)>();
        for (i, t) in targets.iter().enumerate() {
            let mut child = spawn(t)?;
            reader(
                child.stdout.take().context("no pw-record output")?,
                i,
                raw_tx.clone(),
            );
            children.lock().unwrap().push(child);
        }
        drop(raw_tx);

        let n = targets.len();
        thread::spawn(move || {
            if n == 1 {
                for (_, s) in raw_rx {
                    if tx.send(s).is_err() {
                        break;
                    }
                }
                return;
            }
            // Mix two sources. If one stalls, pass the other through alone.
            let mut bufs = [VecDeque::<f32>::new(), VecDeque::<f32>::new()];
            for (i, s) in raw_rx {
                bufs[i].extend(s);
                let ready = bufs[0].len().min(bufs[1].len());
                let out: Vec<f32> = if ready >= READ_SAMPLES {
                    let (a, b) = bufs.split_at_mut(1);
                    a[0].drain(..ready)
                        .zip(b[0].drain(..ready))
                        .map(|(x, y)| (x + y).clamp(-1.0, 1.0))
                        .collect()
                } else if let Some(full) = bufs.iter_mut().find(|b| b.len() > 8 * READ_SAMPLES) {
                    full.drain(..).collect()
                } else {
                    continue;
                };
                if tx.send(out).is_err() {
                    break;
                }
            }
        });
        Ok((Capture { children, stop }, rx))
    }

    /// Records one app's output stream, re-attaching when the app restarts or
    /// opens a new stream (e.g. a new browser tab). While the app plays
    /// nothing, silence is sent so sentences still get closed.
    fn follow_app(
        app: String,
        children: Arc<Mutex<Vec<Child>>>,
        stop: Arc<AtomicBool>,
        tx: Sender<Vec<f32>>,
    ) {
        thread::spawn(move || {
            let mut current: Option<u64> = None;
            let (raw_tx, raw_rx) = mpsc::channel::<(usize, Vec<f32>)>();
            let mut idle_ticks = 0u32;
            while !stop.load(Ordering::SeqCst) {
                let serials = app_stream_serials(&app);
                let alive = children
                    .lock()
                    .unwrap()
                    .iter_mut()
                    .all(|c| matches!(c.try_wait(), Ok(None)));
                let still_there = current.is_some_and(|s| serials.contains(&s));
                if !(still_there && alive) {
                    for mut c in children.lock().unwrap().drain(..) {
                        let _ = c.kill();
                        let _ = c.wait();
                    }
                    current = None;
                    if let Some(&serial) = serials.first()
                        && let Ok(mut child) = spawn(&Target::Stream(serial))
                        && let Some(out) = child.stdout.take()
                    {
                        reader(out, 0, raw_tx.clone());
                        children.lock().unwrap().push(child);
                        current = Some(serial);
                    }
                }
                // Forward audio for ~1 s, then check the app's streams again.
                let mut got = false;
                let deadline = std::time::Instant::now() + Duration::from_secs(1);
                while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
                    match raw_rx.recv_timeout(left) {
                        Ok((_, s)) => {
                            got = true;
                            if tx.send(s).is_err() {
                                return;
                            }
                        }
                        Err(_) => break,
                    }
                }
                if got {
                    idle_ticks = 0;
                } else if idle_ticks < 5 {
                    idle_ticks += 1;
                    // App silent or gone: feed 1 s of silence so the current sentence ends.
                    if tx.send(vec![0.0; 16_000]).is_err() {
                        return;
                    }
                }
            }
        });
    }
}
