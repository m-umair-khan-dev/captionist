//! captionistd: the Captionist caption service.
//!
//! Started on demand over D-Bus (by the GNOME extension or the settings app).
//! The main thread runs a GLib loop that owns GSettings and the pipeline;
//! D-Bus calls and worker threads talk to it through `Cmd` messages.

mod history;
mod reporting;
mod service;
mod shared;

use anyhow::{Context, Result};
use captionist_core::accel::Kind;
use captionist_core::catalog::{ModelEntry, Role};
use captionist_core::listing::{Fit, SortBy};
use captionist_engine::capture::Source;
use captionist_engine::pipeline::{Event, Pipeline, PipelineConfig};
use captionist_engine::text::PostProcessor;
use captionist_engine::{accel, paths};
use gio::prelude::*;
use shared::{Cmd, Shared};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

const SCHEMA: &str = "io.github.captionist.Captionist";

fn unix_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

struct Controller {
    shared: Arc<Shared>,
    settings: gio::Settings,
    pipeline: Option<Pipeline>,
    running_key: String,
}

impl Controller {
    fn stop(&mut self) {
        if let Some(p) = self.pipeline.take() {
            p.stop();
        }
        self.running_key.clear();
        *self.shared.active.lock().unwrap() = (None, None);
    }

    /// Picks the live and correction models from the settings and what's installed.
    fn choose(&self) -> (Option<ModelEntry>, Option<ModelEntry>) {
        let s = &self.shared;
        let installed = |id: &str| s.catalog.get(id).filter(|m| s.is_installed(m)).cloned();
        let refine_on = self.settings.boolean("refine-enabled");

        if self.settings.string("model-mode") == "manual" {
            let live = installed(&self.settings.string("live-model"));
            let refine = if refine_on {
                installed(&self.settings.string("refine-model"))
            } else {
                None
            };
            if live.is_some() {
                return (live, refine);
            }
            // Manual choice not installed: fall through to automatic.
        }

        let report = s.report(SortBy::Recommended);
        // Rows are sorted recommended-first, then by fit and accuracy.
        let pick = |role: Role| {
            report
                .rows
                .iter()
                .filter(|r| r.role == role && r.installed)
                .find(|r| r.fit != Fit::TooSlow)
                .or_else(|| report.rows.iter().find(|r| r.role == role && r.installed))
                .and_then(|r| installed(&r.id))
        };
        let live = pick(Role::Live);
        let refine = if refine_on { pick(Role::Refine) } else { None };
        (live, refine)
    }

    fn apply(&mut self) {
        let shared = self.shared.clone();
        if !self.settings.boolean("enabled") {
            self.stop();
            shared.set_status("off", "", None, None);
            return;
        }
        let (live, refine) = self.choose();
        let Some(live) = live else {
            self.stop();
            let msg = if shared.busy.lock().unwrap().is_some() {
                "Setting up speech models…"
            } else {
                "No speech model installed. Open Captionist Settings → Models."
            };
            shared.set_status("setup-needed", msg, None, None);
            return;
        };
        let source = self.settings.string("source").to_string();
        let app = self.settings.string("source-app").to_string();
        let key = format!(
            "{}|{:?}|{source}|{app}",
            live.id,
            refine.as_ref().map(|m| &m.id)
        );
        if key == self.running_key && self.pipeline.is_some() {
            return;
        }
        self.stop();

        let Some(vad) = paths::data_file("silero_vad.onnx") else {
            shared.set_status(
                "error",
                "Speech detector (silero_vad.onnx) is not installed.",
                None,
                None,
            );
            return;
        };
        let cfg = PipelineConfig {
            live: live.clone(),
            refine: refine.clone(),
            models_root: shared.models_root.clone(),
            vad_model: vad,
            threads: shared.hw.lock().unwrap().threads_per_pass(),
            source: Source::parse(&source, &app),
            post: shared.post.clone(),
            // Only models that passed a test on the accelerator run there.
            refine_provider: match (accel::active(), refine.as_ref()) {
                (Some(k), Some(m)) if shared.runs_on_accel(&m.id) => {
                    accel::provider(k).unwrap_or_else(|_| "cpu".into())
                }
                _ => "cpu".into(),
            },
        };
        let (ln, rn) = (live.name.clone(), refine.as_ref().map(|m| m.name.clone()));
        shared.set_status(
            "loading",
            "Loading speech models…",
            Some(&ln),
            rn.as_deref(),
        );
        let s2 = shared.clone();
        let started = Pipeline::start(cfg, move |ev| match ev {
            Event::Partial { id, text } => s2.emit("Partial", &(id, text)),
            Event::Final {
                id,
                text,
                start,
                end,
            } => {
                if !text.is_empty() {
                    s2.add_history(&text, start, end);
                }
                s2.emit("Final", &(id, text));
            }
            Event::Ready => s2.send(Cmd::Ready),
            Event::Error(e) => {
                eprintln!("pipeline: {e}");
                s2.send(Cmd::PipelineFailed(e));
            }
        });
        match started {
            Ok(p) => {
                *shared.run_started.lock().unwrap() = unix_now();
                self.pipeline = Some(p);
                self.running_key = key;
                *shared.active.lock().unwrap() = (Some(live.id), refine.map(|m| m.id));
                shared.models_changed();
            }
            Err(e) => shared.set_status("error", &format!("{e:#}"), None, None),
        }
    }

    /// Vocabulary, replacements and profanity filter: applied live, no restart.
    fn update_post(&self) {
        let s = &self.settings;
        let vocab: Vec<String> = s.strv("vocabulary").iter().map(|v| v.to_string()).collect();
        let repl: Vec<(String, String)> = s
            .value("replacements")
            .get::<std::collections::HashMap<String, String>>()
            .unwrap_or_default()
            .into_iter()
            .collect();
        let p = PostProcessor::new(&vocab, &repl, s.boolean("profanity-filter"));
        *self.shared.post.write().unwrap() = p;
    }

    fn update_history(&self) {
        let mode = history::Mode::parse(&self.settings.string("history-mode"));
        self.shared
            .history
            .lock()
            .unwrap()
            .configure(mode, self.settings.int("history-days"));
        self.shared.emit("HistoryChanged", &());
    }

    /// Opt-in crash reports follow the `crash-reports` setting ("ask" = off).
    fn update_reporting(&self) {
        let on = self.settings.string("crash-reports") == "on";
        let hw = self.shared.hw.lock().unwrap().clone();
        let tags = [
            ("distro", reporting::distro()),
            ("gnome", reporting::gnome_version()),
            ("cpu_cores", hw.cpu.physical_cores.to_string()),
            ("ram_gb", format!("{}", hw.memory.total_mb / 1024)),
            (
                "accelerator",
                accel::active()
                    .map(|k| k.id().to_string())
                    .unwrap_or_else(|| "cpu".into()),
            ),
        ];
        self.shared.reporter.set_enabled(on, &tags);
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Apply => self.apply(),
            Cmd::UpdatePost => self.update_post(),
            Cmd::UpdateHistory => self.update_history(),
            Cmd::UpdateReporting => self.update_reporting(),
            Cmd::Restart => {
                let want = Kind::parse(&self.settings.string("accelerator"));
                *self.shared.accel_setting.lock().unwrap() = want;
                let hw = self.shared.hw.lock().unwrap().clone();
                let ready = want
                    .and_then(|k| captionist_core::accel::pack_for(k, &hw))
                    .is_some_and(|p| accel::pack_installed(p.id));
                if want == accel::active() || (want.is_some() && !ready) {
                    // Nothing to switch yet (e.g. pack still downloading).
                    self.shared.models_changed();
                    return;
                }
                // Exit; systemd restarts the service (Restart=on-failure) and it
                // re-launches itself with or without the OpenVINO runtime.
                eprintln!("restarting to switch acceleration");
                self.stop();
                reporting::mark_clean_exit(); // a planned restart, not a crash
                std::process::exit(75);
            }
            Cmd::Ready => {
                let a = self.shared.active.lock().unwrap().clone();
                let name = |id: &Option<String>| {
                    id.as_ref()
                        .and_then(|i| self.shared.catalog.get(i))
                        .map(|m| m.name.clone())
                };
                let (l, r) = (name(&a.0), name(&a.1));
                self.shared
                    .set_status("listening", "", l.as_deref(), r.as_deref());
            }
            Cmd::PipelineFailed(e) => {
                self.shared.reporter.error("pipeline", &e);
                self.stop();
                self.shared.set_status("error", &e, None, None);
            }
        }
    }
}

/// Runs the service on the selected accelerator's runtime (when its pack is
/// installed) by re-launching this process with the pack first on the library
/// path. Must happen before anything loads ONNX Runtime.
fn select_runtime(settings: &gio::Settings) {
    use std::os::unix::process::CommandExt;
    let hw = captionist_core::hardware::HardwareReport::probe();
    let want = Kind::parse(&settings.string("accelerator")).and_then(|k| {
        captionist_core::accel::pack_for(k, &hw)
            .filter(|p| accel::pack_installed(p.id))
            .map(|p| (k, p))
    });
    if want.map(|w| w.0) == accel::active() {
        return;
    }
    let mut cmd = std::process::Command::new(std::env::current_exe().expect("own path"));
    cmd.args(std::env::args_os().skip(1));
    match want {
        Some((kind, pack)) => {
            let dir = accel::pack_dir(pack.id);
            cmd.env("LD_LIBRARY_PATH", dir)
                .env(accel::ENV_ACTIVE, kind.id());
        }
        None => {
            cmd.env_remove(accel::ENV_ACTIVE)
                .env_remove("LD_LIBRARY_PATH");
        }
    }
    let err = cmd.exec();
    eprintln!("could not switch runtime: {err}");
}

/// `captionistd measure <model> <provider> <label>`: one speed test, printed as
/// JSON. Used by the service to test accelerators in a separate process.
fn measure_cmd(args: &[String]) -> Result<()> {
    let [model, provider, label] = args else {
        anyhow::bail!("usage: captionistd measure <model> <provider> <label>")
    };
    let catalog = captionist_core::catalog::Catalog::embedded();
    let m = catalog.get(model).context("unknown model")?;
    let clip = captionist_engine::calibrate::load_clip(
        &paths::data_file("calibration.wav").context("calibration clip missing")?,
    )?;
    let threads = captionist_core::hardware::HardwareReport::probe().threads_per_pass();
    let r = captionist_engine::calibrate::measure(
        m,
        &paths::models_root(),
        &clip,
        threads,
        provider,
        label,
    )?;
    println!("{}", serde_json::to_string(&r)?);
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("measure") {
        return measure_cmd(&args[2..]);
    }
    let settings = gio::Settings::new(SCHEMA);
    select_runtime(&settings);
    eprintln!(
        "runtime: {}",
        accel::active()
            .map(|k| k.label())
            .unwrap_or("processor (CPU)")
    );

    let (tx, rx) = mpsc::channel::<Cmd>();
    let shared = Arc::new(Shared::new(tx.clone()));

    let conn = zbus::blocking::connection::Builder::session()?
        .name(shared::NAME)?
        .serve_at(
            shared::PATH,
            service::Service {
                shared: shared.clone(),
            },
        )?
        .build()
        .context("could not claim the D-Bus name (is captionistd already running?)")?;
    let _ = shared.conn.set(conn);

    *shared.accel_setting.lock().unwrap() = Kind::parse(&settings.string("accelerator"));
    let ctl = Rc::new(RefCell::new(Controller {
        shared: shared.clone(),
        settings: settings.clone(),
        pipeline: None,
        running_key: String::new(),
    }));

    {
        let tx = tx.clone();
        settings.connect_changed(None, move |_, key| {
            // Appearance keys are for the overlay only.
            let cmd = match key {
                "enabled" | "source" | "source-app" | "model-mode" | "live-model"
                | "refine-model" | "refine-enabled" => Cmd::Apply,
                "vocabulary" | "replacements" | "profanity-filter" => Cmd::UpdatePost,
                "history-mode" | "history-days" => Cmd::UpdateHistory,
                "accelerator" => Cmd::Restart,
                "crash-reports" => Cmd::UpdateReporting,
                _ => return,
            };
            let _ = tx.send(cmd);
        });
    }
    {
        let ctl = ctl.clone();
        glib::timeout_add_local(Duration::from_millis(50), move || {
            while let Ok(cmd) = rx.try_recv() {
                ctl.borrow_mut().handle(cmd);
            }
            glib::ControlFlow::Continue
        });
    }

    // First run: time the calibration model so speeds can be estimated.
    if !shared.has_calibration() {
        let s = shared.clone();
        std::thread::spawn(move || {
            if let Err(e) = s.recheck(false) {
                eprintln!("first-run check failed: {e:#}");
                s.reporter
                    .error("setup", &format!("first-run check: {e:#}"));
            }
        });
    }
    // First start on an accelerator: time the correction models on it.
    if let Some(kind) = accel::active().filter(|k| !shared.has_accel_results(*k)) {
        let s = shared.clone();
        std::thread::spawn(move || {
            if let Err(e) = s.measure_accel_all() {
                eprintln!("accelerator speed test failed: {e:#}");
                s.emit("Error", &(format!("{} test failed: {e:#}", kind.label()),));
            }
        });
    }
    ctl.borrow().update_post();
    ctl.borrow().update_history();
    ctl.borrow().update_reporting();

    // Did the last run end unexpectedly (e.g. a crash inside the C++ speech engine)?
    if reporting::check_and_mark_running() {
        let log = reporting::recent_log(30);
        eprintln!("previous run ended unexpectedly");
        shared.reporter.error(
            "crash",
            &format!("Caption service ended unexpectedly. Last log lines:\n{log}"),
        );
    }
    // A normal stop (logout, systemctl stop) sends SIGTERM: exit cleanly, so
    // it isn't mistaken for a crash next time.
    let main_loop = glib::MainLoop::new(None, false);
    let stop_requested = Arc::new(std::sync::atomic::AtomicBool::new(false));
    for sig in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT] {
        signal_hook::flag::register(sig, stop_requested.clone())?;
    }
    {
        let (ml, flag) = (main_loop.clone(), stop_requested.clone());
        glib::timeout_add_local(Duration::from_millis(100), move || {
            if flag.load(std::sync::atomic::Ordering::Relaxed) {
                ml.quit();
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Continue
        });
    }
    let _ = tx.send(Cmd::Apply);

    eprintln!("captionistd {} ready", env!("CARGO_PKG_VERSION"));
    main_loop.run();
    ctl.borrow_mut().stop();
    reporting::mark_clean_exit();
    eprintln!("captionistd stopped");
    Ok(())
}
