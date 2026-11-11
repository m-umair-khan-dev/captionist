//! Two-pass live captioning.
//!
//! capture ─► [live thread] streaming model → Partial(id, draft)
//!                  │ Silero VAD marks where each sentence ends
//!                  ▼
//!            [refine thread] offline model re-reads the sentence → Final(id, text)
//!
//! Each sentence ("segment") has an id. The overlay shows `Partial` text as a
//! draft and swaps in the `Final` text for the same id when it arrives.

use crate::asr::{self, Recognizer};
use crate::capture::{Capture, Source};
use crate::text;
use anyhow::{Context, Result, bail};
use captionist_core::catalog::ModelEntry;
use sherpa_onnx::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub enum Event {
    /// Draft text for a sentence that is still being spoken.
    Partial {
        id: u32,
        text: String,
    },
    /// Final text for a sentence. Empty text means "remove this sentence".
    /// `start`/`end`: seconds since the pipeline started.
    Final {
        id: u32,
        text: String,
        start: f64,
        end: f64,
    },
    /// Models loaded, listening.
    Ready,
    Error(String),
}

#[derive(Clone)]
pub struct PipelineConfig {
    pub live: ModelEntry,
    pub refine: Option<ModelEntry>,
    pub models_root: PathBuf,
    pub vad_model: PathBuf,
    pub threads: i32,
    pub source: Source,
    /// Vocabulary, replacements, profanity filter. Updated live by the caller.
    pub post: Arc<RwLock<text::PostProcessor>>,
    /// ONNX Runtime provider for the correction model ("cpu" or an OpenVINO string).
    pub refine_provider: String,
}

/// A finished sentence waiting for correction.
struct Segment {
    id: u32,
    samples: Vec<f32>,
    draft: String,
    start: f64,
    end: f64,
}

pub struct Pipeline {
    stop: Arc<AtomicBool>,
    handles: Vec<JoinHandle<()>>,
}

/// Sentences waiting for correction beyond this are shown as drafts instead,
/// so captions never fall far behind on a slow PC.
const MAX_REFINE_BACKLOG: usize = 3;
/// A draft with no detected speech for this long is finalised as-is.
const STALE_DRAFT: Duration = Duration::from_millis(2500);
const SR: i32 = asr::SAMPLE_RATE;
/// Extra audio given to the correction model before each sentence.
const PRE_ROLL: i64 = (SR as i64) / 2;
/// How much recent audio to keep for cutting out sentences (40 s).
const HISTORY_SAMPLES: usize = 40 * SR as usize;

impl Pipeline {
    pub fn start(
        cfg: PipelineConfig,
        on_event: impl Fn(Event) + Send + Sync + 'static,
    ) -> Result<Pipeline> {
        if !cfg.vad_model.exists() {
            bail!("speech detector model missing: {}", cfg.vad_model.display());
        }
        let stop = Arc::new(AtomicBool::new(false));
        let emit = Arc::new(on_event);
        let mut handles = vec![];

        // Correction thread.
        let (seg_tx, seg_rx) = mpsc::channel::<Segment>();
        let backlog = Arc::new(AtomicUsize::new(0));
        let refine_enabled = cfg.refine.is_some();
        if let Some(refine) = cfg.refine.clone() {
            let (emit, stop, backlog, cfg) =
                (emit.clone(), stop.clone(), backlog.clone(), cfg.clone());
            handles.push(thread::spawn(move || {
                let rec = match asr::build(
                    &refine,
                    &cfg.models_root,
                    cfg.threads,
                    &cfg.refine_provider,
                ) {
                    Ok(r) => r,
                    Err(e) => {
                        emit(Event::Error(format!(
                            "could not load {}: {e:#}",
                            refine.name
                        )));
                        // Fall back to passing drafts through.
                        for s in seg_rx {
                            emit(Event::Final {
                                id: s.id,
                                text: s.draft,
                                start: s.start,
                                end: s.end,
                            });
                        }
                        return;
                    }
                };
                for s in seg_rx {
                    let pending = backlog.fetch_sub(1, Ordering::SeqCst) - 1;
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    if pending >= MAX_REFINE_BACKLOG {
                        emit(Event::Final {
                            id: s.id,
                            text: s.draft,
                            start: s.start,
                            end: s.end,
                        });
                        continue;
                    }
                    // A little silence around the sentence helps the model.
                    let pad = vec![0.0f32; (SR as usize) / 4];
                    let audio: Vec<f32> = pad
                        .iter()
                        .chain(s.samples.iter())
                        .chain(pad.iter())
                        .copied()
                        .collect();
                    let text = match rec.transcribe(&refine, &audio) {
                        Ok(t) => finish_text(&t.text, &cfg.post),
                        Err(_) => s.draft,
                    };
                    emit(Event::Final {
                        id: s.id,
                        text,
                        start: s.start,
                        end: s.end,
                    });
                }
            }));
        }

        // Capture + live thread.
        {
            let (emit, stop) = (emit.clone(), stop.clone());
            handles.push(thread::spawn(move || {
                if let Err(e) = live_loop(&cfg, &stop, &*emit, seg_tx, &backlog, refine_enabled) {
                    emit(Event::Error(format!("{e:#}")));
                }
            }));
        }
        Ok(Pipeline { stop, handles })
    }

    /// Stops capture and both threads. Blocks until they have exited.
    pub fn stop(self) {
        self.stop.store(true, Ordering::SeqCst);
        for h in self.handles {
            let _ = h.join();
        }
    }
}

fn finish_text(raw: &str, post: &RwLock<text::PostProcessor>) -> String {
    let t = text::clean(raw);
    if t.is_empty() {
        return t;
    }
    post.read().map(|p| p.apply(&t)).unwrap_or(t)
}

fn live_loop(
    cfg: &PipelineConfig,
    stop: &AtomicBool,
    emit: &(dyn Fn(Event) + Send + Sync),
    seg_tx: mpsc::Sender<Segment>,
    backlog: &AtomicUsize,
    refine_enabled: bool,
) -> Result<()> {
    // The live model is small: one thread leaves the cores to the correction model.
    let live_threads = if cfg.refine.is_some() {
        (cfg.threads / 2).max(1)
    } else {
        cfg.threads
    };
    let rec = asr::build(&cfg.live, &cfg.models_root, live_threads, "cpu")
        .with_context(|| format!("could not load {}", cfg.live.name))?;
    let Recognizer::Online(rec) = rec else {
        bail!("{} is not a streaming model", cfg.live.name)
    };
    let stream = rec.create_stream();
    for (k, v) in &cfg.live.stream_options {
        stream.set_option(k, v);
    }

    let vad_cfg = VadModelConfig {
        silero_vad: SileroVadModelConfig {
            model: Some(cfg.vad_model.to_string_lossy().into()),
            threshold: 0.5,
            min_silence_duration: 0.5,
            min_speech_duration: 0.25,
            window_size: 512,
            max_speech_duration: 12.0,
        },
        sample_rate: SR,
        num_threads: 1,
        provider: Some("cpu".into()),
        ..Default::default()
    };
    let vad = VoiceActivityDetector::create(&vad_cfg, 30.0)
        .context("could not start the speech detector")?;

    let (_capture, audio) = Capture::start(cfg.source.clone())?;
    emit(Event::Ready);

    let mut id: u32 = 1;
    let mut draft = String::new();
    let mut vad_buf: Vec<f32> = Vec::with_capacity(2048);
    // Recent audio exactly as fed to the VAD, so each sentence can be cut out
    // with some extra audio before it (the VAD trims the very start of speech).
    let mut history: std::collections::VecDeque<f32> = std::collections::VecDeque::new();
    let mut history_base: i64 = 0; // absolute sample index of history[0]
    let mut last_speech = Instant::now();
    // Seconds (since start) when the current draft began, for history timing.
    let mut draft_start: f64 = 0.0;
    let secs = |samples: i64| samples as f64 / SR as f64;

    let tail = vec![0.0f32; (cfg.live.tail_padding_s * SR as f32) as usize];
    // Closes the current sentence: sends it for correction (or finalises the draft).
    let finish =
        |id: &mut u32, draft: &mut String, samples: Option<Vec<f32>>, start: f64, end: f64| {
            if samples.is_some() {
                // Flush the live model with silence so its last words land in this
                // sentence instead of leaking into the next one.
                stream.accept_waveform(SR, &tail);
                while rec.is_ready(&stream) {
                    rec.decode(&stream);
                }
                let flushed = rec
                    .get_result(&stream)
                    .map(|r| finish_text(&r.text, &cfg.post))
                    .unwrap_or_default();
                if !flushed.is_empty() && flushed != *draft {
                    *draft = flushed;
                    emit(Event::Partial {
                        id: *id,
                        text: draft.clone(),
                    });
                }
            } else if draft.split_whitespace().count() < 3 {
                // A short draft with no detected speech is noise, not a sentence.
                emit(Event::Final {
                    id: *id,
                    text: String::new(),
                    start,
                    end,
                });
                rec.reset(&stream);
                *id += 1;
                draft.clear();
                return;
            }
            match samples {
                Some(s) if refine_enabled => {
                    backlog.fetch_add(1, Ordering::SeqCst);
                    let _ = seg_tx.send(Segment {
                        id: *id,
                        samples: s,
                        draft: draft.clone(),
                        start,
                        end,
                    });
                }
                _ if !draft.is_empty() => emit(Event::Final {
                    id: *id,
                    text: draft.clone(),
                    start,
                    end,
                }),
                _ => return,
            }
            rec.reset(&stream);
            *id += 1;
            draft.clear();
        };

    while !stop.load(Ordering::SeqCst) {
        let chunk = match audio.recv_timeout(Duration::from_millis(200)) {
            Ok(c) => c,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => bail!("audio capture stopped"),
        };

        stream.accept_waveform(SR, &chunk);
        while rec.is_ready(&stream) {
            rec.decode(&stream);
        }
        let text = rec
            .get_result(&stream)
            .map(|r| finish_text(&r.text, &cfg.post))
            .unwrap_or_default();
        if !text.is_empty() && text != draft {
            if draft.is_empty() {
                draft_start = secs(history_base + history.len() as i64);
            }
            draft = text;
            emit(Event::Partial {
                id,
                text: draft.clone(),
            });
        }

        vad_buf.extend_from_slice(&chunk);
        let whole = vad_buf.len() / 512 * 512;
        for w in vad_buf[..whole].as_chunks::<512>().0 {
            vad.accept_waveform(w);
            history.extend(w);
        }
        vad_buf.drain(..whole);
        let excess = history.len().saturating_sub(HISTORY_SAMPLES);
        if excess > 0 {
            history.drain(..excess);
            history_base += excess as i64;
        }
        if vad.detected() {
            last_speech = Instant::now();
        }

        while let Some(seg) = vad.front() {
            let seg_start = seg.start();
            let from = (seg_start - PRE_ROLL).max(history_base);
            let to = seg.start() + seg.n() as i64;
            let samples: Vec<f32> = if to > from && to - history_base <= history.len() as i64 {
                history
                    .range((from - history_base) as usize..(to - history_base) as usize)
                    .copied()
                    .collect()
            } else {
                seg.samples().to_vec()
            };
            vad.pop();
            finish(
                &mut id,
                &mut draft,
                Some(samples),
                secs(seg_start),
                secs(to),
            );
        }
        if !draft.is_empty() && !vad.detected() && last_speech.elapsed() > STALE_DRAFT {
            let now = secs(history_base + history.len() as i64);
            finish(&mut id, &mut draft, None, draft_start, now);
        }
    }
    Ok(())
}
