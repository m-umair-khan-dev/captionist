//! State shared between the D-Bus handlers, worker threads and the main loop.

use captionist_core::catalog::{Catalog, ModelEntry};
use captionist_core::hardware::HardwareReport;
use captionist_core::listing::{self, ModelsReport, SortBy};
use captionist_core::recommend::{self, Limits, SpeedResult};
use captionist_engine::{accel, calibrate, download, paths};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use crate::history::History;
use captionist_core::history::Entry;
use captionist_engine::text::PostProcessor;

pub const NAME: &str = "io.github.m_umair_khan_dev.Captionist.Daemon";
pub const PATH: &str = "/io/github/m_umair_khan_dev/Captionist/Daemon";
pub const IFACE: &str = "io.github.m_umair_khan_dev.Captionist.Daemon1";

/// Requests for the main thread (which owns GSettings and the pipeline).
#[derive(Debug)]
pub enum Cmd {
    /// Settings or installed models changed: start/stop/restart as needed.
    Apply,
    /// The running pipeline failed; drop it.
    PipelineFailed(String),
    Ready,
    /// Vocabulary / replacements / profanity filter changed.
    UpdatePost,
    /// Crash-reports setting changed.
    UpdateReporting,
    /// Accelerator setting changed: restart the service with/without OpenVINO.
    Restart,
    /// History settings changed.
    UpdateHistory,
}

#[derive(Serialize, Deserialize, Default)]
struct SpeedCache {
    fingerprint: String,
    results: Vec<SpeedResult>,
}

pub struct Shared {
    pub catalog: Catalog,
    pub models_root: PathBuf,
    pub hw: Mutex<HardwareReport>,
    pub measured: Mutex<Vec<SpeedResult>>,
    pub busy: Mutex<Option<String>>,
    pub downloading: Mutex<BTreeMap<String, f64>>,
    /// (live model id, refine model id) currently running.
    pub active: Mutex<(Option<String>, Option<String>)>,
    pub status: Mutex<String>,
    pub conn: OnceLock<gio::DBusConnection>,
    pub cmd: Mutex<Sender<Cmd>>,
    /// Vocabulary / replacements / profanity filter, shared with the pipeline.
    pub post: Arc<RwLock<PostProcessor>>,
    pub history: Mutex<History>,
    /// Unix time when the running pipeline started (sentence times are relative to it).
    pub run_started: Mutex<f64>,
    /// The `accelerator` setting (mirrored from GSettings by the main thread).
    pub accel_setting: Mutex<Option<captionist_core::accel::Kind>>,
    /// Diagnostics and opt-in crash reports.
    pub reporter: crate::reporting::Reporter,
}

impl Shared {
    pub fn new(cmd: Sender<Cmd>) -> Self {
        let hw = HardwareReport::probe();
        let cache: SpeedCache = std::fs::read_to_string(Self::cache_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let measured = if cache.fingerprint == hw.fingerprint {
            cache.results
        } else {
            vec![]
        };
        Shared {
            catalog: Catalog::embedded(),
            models_root: paths::models_root(),
            hw: Mutex::new(hw),
            measured: Mutex::new(measured),
            busy: Mutex::new(None),
            downloading: Mutex::new(BTreeMap::new()),
            active: Mutex::new((None, None)),
            status: Mutex::new(status_json("off", "", None, None)),
            conn: OnceLock::new(),
            cmd: Mutex::new(cmd),
            post: Arc::new(RwLock::new(PostProcessor::default())),
            history: Mutex::new(History::new()),
            run_started: Mutex::new(0.0),
            accel_setting: Mutex::new(None),
            reporter: crate::reporting::Reporter::new(),
        }
    }

    fn cache_path() -> PathBuf {
        paths::state_dir().join("speed.json")
    }

    fn save_measured(&self) {
        let cache = SpeedCache {
            fingerprint: self.hw.lock().unwrap().fingerprint.clone(),
            results: self.measured.lock().unwrap().clone(),
        };
        let _ = std::fs::create_dir_all(paths::state_dir());
        let _ = std::fs::write(
            Self::cache_path(),
            serde_json::to_string_pretty(&cache).unwrap_or_default(),
        );
    }

    pub fn send(&self, c: Cmd) {
        let _ = self.cmd.lock().unwrap().send(c);
    }

    /// Broadcasts a signal. `body` is a tuple of the signal's arguments.
    /// Safe to call from any thread.
    pub fn emit<B: glib::variant::ToVariant>(&self, signal: &str, body: &B) {
        if let Some(conn) = self.conn.get() {
            let _ = conn.emit_signal(None, PATH, IFACE, signal, Some(&body.to_variant()));
        }
    }

    pub fn set_status(&self, state: &str, message: &str, live: Option<&str>, refine: Option<&str>) {
        let s = status_json(state, message, live, refine);
        *self.status.lock().unwrap() = s.clone();
        self.emit("StatusChanged", &(s,));
    }

    /// Records a finished sentence in the history.
    pub fn add_history(&self, text: &str, start: f64, end: f64) {
        let t0 = *self.run_started.lock().unwrap();
        self.history.lock().unwrap().add(Entry {
            at: t0 + start,
            duration: (end - start).max(0.0),
            text: text.to_string(),
        });
    }

    pub fn models_changed(&self) {
        self.emit("ModelsChanged", &());
    }

    pub fn is_installed(&self, m: &ModelEntry) -> bool {
        m.is_installed(&self.models_root)
    }

    pub fn report(&self, sort: SortBy) -> ModelsReport {
        let hw = self.hw.lock().unwrap().clone();
        let all = self.measured.lock().unwrap().clone();
        let active_kind = accel::active();
        let active_label = active_kind.map(captionist_core::accel::result_label);
        // Speeds shown = how models run right now: on the processor, or on the
        // accelerator for correction models when one is active.
        let on_accel = |id: &str| self.runs_on_accel(id) && active_label.is_some();
        let measured: Vec<SpeedResult> = all
            .iter()
            .filter(|r| {
                if r.provider == "cpu" {
                    !on_accel(&r.model_id)
                } else {
                    active_label.as_ref() == Some(&r.provider) && r.failed.is_none()
                }
            })
            .cloned()
            .collect();
        let factor = listing::calibration_factor(&self.catalog, &all);
        let speeds =
            listing::effective_speeds(&self.catalog, &measured, factor, hw.threads_per_pass());
        let only: Vec<_> = speeds.iter().map(|(r, _)| r.clone()).collect();
        let mut rec = recommend::recommend(&hw, &self.catalog, &only, Limits::default());
        let mut rows = listing::rows(
            &self.catalog,
            &speeds,
            &rec,
            |m| self.is_installed(m),
            Limits::default(),
        );
        listing::sort_rows(&mut rows, sort);

        let setting = *self.accel_setting.lock().unwrap();
        let mut options =
            captionist_core::accel::scan(&hw, &all, accel::pack_installed, setting, active_kind);
        for o in &mut options {
            o.results = o
                .results
                .iter()
                .map(|(id, c, a)| {
                    (
                        self.catalog
                            .get(id)
                            .map(|m| m.name.clone())
                            .unwrap_or_else(|| id.clone()),
                        *c,
                        *a,
                    )
                })
                .collect();
        }
        if let Some(sug) = captionist_core::accel::suggestion(&options) {
            rec.notes.push(format!(
                "{}: {}. See Acceleration below.",
                sug.device,
                sug.message.to_lowercase()
            ));
        }
        let active = self.active.lock().unwrap().clone();
        ModelsReport {
            hardware: hw,
            factor,
            busy: self.busy.lock().unwrap().clone(),
            recommendation: rec,
            rows,
            active_live: active.0,
            active_refine: active.1,
            downloading: self
                .downloading
                .lock()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), *v))
                .collect(),
            accel: options,
        }
    }

    /// Times installed correction models on the active accelerator (all of
    /// them, or only those not tested on it yet). Blocking. A model runs on
    /// the accelerator only after passing this test.
    pub fn measure_accel(&self, only_untested: bool) -> anyhow::Result<()> {
        let Some(kind) = accel::active() else {
            return Ok(());
        };
        let provider = accel::provider(kind)?;
        let label = captionist_core::accel::result_label(kind);
        let models: Vec<_> = self
            .catalog
            .by_role(captionist_core::catalog::Role::Refine)
            .filter(|m| self.is_installed(m))
            .filter(|m| !only_untested || !self.tested_on_accel(&m.id, kind))
            .cloned()
            .collect();
        if models.is_empty() {
            return Ok(());
        }
        for m in models {
            // Make sure the processor result exists for comparison.
            if !self
                .measured
                .lock()
                .unwrap()
                .iter()
                .any(|r| r.model_id == m.id && r.provider == "cpu")
            {
                self.measure(&m)?;
            }
            self.measure_on(&m, &provider, &label)?;
        }
        self.send(Cmd::Apply);
        Ok(())
    }

    /// Whether model `id` passed a speed test on the active accelerator (and
    /// may therefore run on it).
    pub fn runs_on_accel(&self, id: &str) -> bool {
        let Some(kind) = accel::active() else {
            return false;
        };
        let label = captionist_core::accel::result_label(kind);
        self.measured
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.model_id == id && r.provider == label && r.failed.is_none())
    }

    /// Whether model `id` has a result (pass or fail) on accelerator `kind`.
    fn tested_on_accel(&self, id: &str, kind: captionist_core::accel::Kind) -> bool {
        let label = captionist_core::accel::result_label(kind);
        self.measured
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.model_id == id && r.provider == label)
    }

    /// Tests correction models not yet tried on the active accelerator, e.g.
    /// one downloaded after the accelerator was turned on. Errors are reported
    /// here, so a failed test never fails the download that triggered it.
    pub fn test_new_models_on_accel(&self) {
        let Some(kind) = accel::active() else {
            return;
        };
        if let Err(e) = self.measure_accel(true) {
            eprintln!("accelerator speed test failed: {e:#}");
            self.reporter
                .error("accelerator", &format!("{} test: {e:#}", kind.label()));
            self.emit("Error", &(format!("{} test failed: {e:#}", kind.label()),));
        }
    }

    fn set_busy(&self, msg: Option<String>) {
        *self.busy.lock().unwrap() = msg;
        self.models_changed();
    }

    /// Downloads a model (if needed), with progress signals. Blocking.
    pub fn ensure_downloaded(&self, m: &ModelEntry) -> anyhow::Result<()> {
        if self.is_installed(m) {
            return Ok(());
        }
        {
            let mut d = self.downloading.lock().unwrap();
            if d.contains_key(&m.id) {
                anyhow::bail!("{} is already downloading", m.name);
            }
            d.insert(m.id.clone(), 0.0);
        }
        self.models_changed();
        let id = m.id.clone();
        let res = download::download(m, &self.models_root, |p| {
            self.downloading.lock().unwrap().insert(id.clone(), p);
            self.emit("DownloadProgress", &(id.clone(), p));
        });
        self.downloading.lock().unwrap().remove(&m.id);
        self.models_changed();
        res
    }

    /// Times `m` on this PC and stores the result. Blocking.
    pub fn measure(&self, m: &ModelEntry) -> anyhow::Result<()> {
        self.measure_on(m, "cpu", "cpu")
    }

    /// Runs a speed test in a child process (`captionistd measure …`). Accelerator
    /// runtimes can abort the whole process on models they can't handle, so
    /// they never run inside the service until a test has passed.
    fn measure_isolated(
        &self,
        m: &ModelEntry,
        provider: &str,
        label: &str,
        threads: i32,
    ) -> SpeedResult {
        let fail = |msg: String| SpeedResult {
            model_id: m.id.clone(),
            provider: label.into(),
            threads,
            rtf: 0.0,
            peak_rss_mb: 0,
            load_ms: 0,
            chunk_p95_ms: None,
            failed: Some(msg),
        };
        let exe = match std::env::current_exe() {
            Ok(e) => e,
            Err(e) => return fail(e.to_string()),
        };
        let out = std::process::Command::new(exe)
            .args(["measure", &m.id, provider, label])
            .output();
        match out {
            Ok(o) => {
                let stdout = String::from_utf8_lossy(&o.stdout);
                match stdout
                    .lines()
                    .find_map(|l| serde_json::from_str::<SpeedResult>(l).ok())
                {
                    Some(r) if o.status.success() => r,
                    _ => {
                        let err = String::from_utf8_lossy(&o.stderr);
                        let last = err
                            .lines()
                            .rfind(|l| !l.trim().is_empty())
                            .unwrap_or("the test process crashed");
                        fail(last.chars().take(300).collect())
                    }
                }
            }
            Err(e) => fail(e.to_string()),
        }
    }

    pub fn measure_on(&self, m: &ModelEntry, provider: &str, label: &str) -> anyhow::Result<()> {
        let clip_path = paths::data_file("calibration.wav")
            .ok_or_else(|| anyhow::anyhow!("calibration clip not installed"))?;
        let clip = calibrate::load_clip(&clip_path)?;
        let threads = self.hw.lock().unwrap().threads_per_pass();
        let on = if label == "cpu" {
            "the processor"
        } else {
            "the accelerator (the first run prepares the model and can take a minute)"
        };
        self.set_busy(Some(format!("Testing {} on {on}…", m.name)));
        let r = if label == "cpu" {
            calibrate::measure(m, &self.models_root, &clip, threads, provider, label)
        } else {
            Ok(self.measure_isolated(m, provider, label, threads))
        };
        self.set_busy(None);
        let r = r?;
        if let Some(f) = &r.failed {
            eprintln!("{} on {label} failed: {f}", m.name);
            self.reporter
                .error("accelerator", &format!("{} on {label}: {f}", m.id));
        }
        {
            let mut ms = self.measured.lock().unwrap();
            ms.retain(|x| !(x.model_id == r.model_id && x.provider == r.provider));
            ms.push(r);
        }
        self.save_measured();
        self.models_changed();
        Ok(())
    }

    /// First-run / "Re-check hardware": probe, download the calibration model,
    /// time it (and any other installed models when `all`).
    pub fn recheck(&self, all: bool) -> anyhow::Result<()> {
        *self.hw.lock().unwrap() = HardwareReport::probe();
        let cal = self
            .catalog
            .calibration_model()
            .ok_or_else(|| anyhow::anyhow!("no calibration model"))?
            .clone();
        self.set_busy(Some(format!(
            "Downloading {} ({} MB) to check this PC…",
            cal.name, cal.download_mb
        )));
        let r = self.ensure_downloaded(&cal);
        if let Err(e) = r {
            self.set_busy(None);
            return Err(e);
        }
        self.measured
            .lock()
            .unwrap()
            .retain(|r| r.provider != "cpu");
        self.measure(&cal)?;
        if all {
            let others: Vec<_> = self
                .catalog
                .models
                .iter()
                .filter(|m| !m.calibration && self.is_installed(m))
                .cloned()
                .collect();
            for m in others {
                self.measure(&m)?;
            }
            self.measure_accel(false)?;
        }
        self.send(Cmd::Apply);
        Ok(())
    }

    pub fn has_calibration(&self) -> bool {
        listing::calibration_factor(&self.catalog, &self.measured.lock().unwrap()).is_some()
    }
}

pub fn status_json(state: &str, message: &str, live: Option<&str>, refine: Option<&str>) -> String {
    serde_json::json!({ "state": state, "message": message, "live": live, "refine": refine })
        .to_string()
}
