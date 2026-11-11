//! D-Bus interface `io.github.m_umair_khan_dev.Captionist.Daemon1` on `io.github.m_umair_khan_dev.Captionist.Daemon`.
//!
//! Signals (emitted from worker threads via `Shared::emit`):
//!   Partial(u id, s text)   Final(u id, s text)   StatusChanged(s json)
//!   DownloadProgress(s id, d fraction)            ModelsChanged()

use crate::shared::Shared;
use captionist_core::listing::SortBy;
use std::sync::Arc;
use std::thread;
use zbus::fdo;

pub struct Service {
    pub shared: Arc<Shared>,
}

fn spawn_job(
    shared: &Arc<Shared>,
    what: &'static str,
    job: impl FnOnce(&Shared) -> anyhow::Result<()> + Send + 'static,
) {
    let s = shared.clone();
    thread::spawn(move || {
        if let Err(e) = job(&s) {
            eprintln!("{what} failed: {e:#}");
            s.reporter.error(what, &format!("{e:#}"));
            s.emit("Error", &(format!("{what} failed: {e:#}"),));
        }
    });
}

#[zbus::interface(name = "io.github.m_umair_khan_dev.Captionist.Daemon1")]
impl Service {
    /// Returns the version. Calling it starts the service (D-Bus activation).
    fn ping(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }

    /// JSON: { state: off|loading|listening|setup-needed|error, message, live, refine }
    fn status(&self) -> String {
        self.shared.status.lock().unwrap().clone()
    }

    /// JSON ModelsReport. sort: recommended|accuracy|speed|download|memory
    fn list_models(&self, sort: String) -> String {
        let sort: SortBy = sort.parse().unwrap_or_default();
        serde_json::to_string(&self.shared.report(sort)).unwrap_or_default()
    }

    /// Re-probe hardware and re-time the calibration model and installed models.
    fn recheck(&self) {
        spawn_job(&self.shared, "Hardware check", |s| s.recheck(true));
    }

    fn download_model(&self, id: String) -> fdo::Result<()> {
        let m = self
            .shared
            .catalog
            .get(&id)
            .cloned()
            .ok_or_else(|| fdo::Error::InvalidArgs(format!("unknown model {id}")))?;
        spawn_job(&self.shared, "Download", move |s| {
            s.ensure_downloaded(&m)?;
            s.measure(&m)?;
            s.send(crate::shared::Cmd::Apply);
            Ok(())
        });
        Ok(())
    }

    /// Downloads whichever recommended models are missing.
    fn download_recommended(&self) {
        spawn_job(&self.shared, "Download", |s| {
            let rec = s.report(SortBy::Recommended).recommendation;
            for c in [rec.live, rec.refine].into_iter().flatten() {
                if let Some(m) = s.catalog.get(&c.model_id).cloned()
                    && !s.is_installed(&m)
                {
                    s.ensure_downloaded(&m)?;
                    s.measure(&m)?;
                }
            }
            s.send(crate::shared::Cmd::Apply);
            Ok(())
        });
    }

    /// Downloads the runtime pack for an accelerator ("intel-gpu", "intel-npu",
    /// "nvidia-gpu"). Progress arrives as DownloadProgress("pack:<id>", fraction).
    fn install_accelerator(&self, kind: String) -> fdo::Result<()> {
        let kind = captionist_core::accel::Kind::parse(&kind)
            .ok_or_else(|| fdo::Error::InvalidArgs(format!("unknown accelerator {kind}")))?;
        let hw = self.shared.hw.lock().unwrap().clone();
        let pack = captionist_core::accel::pack_for(kind, &hw).ok_or_else(|| {
            fdo::Error::Failed("No runtime pack fits this PC's drivers yet".into())
        })?;
        spawn_job(&self.shared, "Accelerator download", move |s| {
            let key = format!("pack:{}", pack.id);
            s.downloading.lock().unwrap().insert(key.clone(), 0.0);
            s.models_changed();
            let r = captionist_engine::accel::install_pack(&pack, |p| {
                s.downloading.lock().unwrap().insert(key.clone(), p);
                s.emit("DownloadProgress", &(key.clone(), p));
            });
            s.downloading.lock().unwrap().remove(&key);
            s.models_changed();
            r?;
            // If this accelerator is already selected, restart onto it.
            if *s.accel_setting.lock().unwrap() == Some(kind) {
                s.send(crate::shared::Cmd::Restart);
            }
            Ok(())
        });
        Ok(())
    }

    fn remove_accelerator(&self, pack_id: String) -> fdo::Result<()> {
        let hw = self.shared.hw.lock().unwrap().clone();
        if let Some(k) = captionist_engine::accel::active()
            && captionist_core::accel::pack_for(k, &hw).is_some_and(|p| p.id == pack_id)
        {
            return Err(fdo::Error::Failed("Turn this accelerator off first".into()));
        }
        captionist_engine::accel::remove_pack(&pack_id)
            .map_err(|e| fdo::Error::Failed(format!("{e:#}")))?;
        self.shared.models_changed();
        Ok(())
    }

    /// JSON list of apps playing audio right now: [{name, media, icon}]
    fn list_apps(&self) -> String {
        serde_json::to_string(&captionist_engine::capture::list_audio_apps())
            .unwrap_or_else(|_| "[]".into())
    }

    /// JSON list of caption history entries: [{at, duration, text}]
    fn get_history(&self) -> String {
        serde_json::to_string(&self.shared.history.lock().unwrap().entries)
            .unwrap_or_else(|_| "[]".into())
    }

    fn clear_history(&self) {
        self.shared.history.lock().unwrap().clear();
        self.shared.emit("HistoryChanged", &());
    }

    /// JSON for "Report a problem": {version, system, hardware, logs, text}.
    /// Scrubbed of user and host names; never contains caption text.
    fn diagnostics_report(&self) -> String {
        use crate::reporting as r;
        let s = &self.shared;
        let hw = s.hw.lock().unwrap().clone();
        let report = s.report(captionist_core::listing::SortBy::Recommended);
        let gpus: Vec<String> = hw
            .gpus
            .iter()
            .map(|g| {
                format!(
                    "{} (compute driver: {})",
                    g.name,
                    if g.compute_runtime { "yes" } else { "no" }
                )
            })
            .collect();
        let hardware = format!(
            "CPU: {} ({} cores)\nRAM: {:.1} GB\nGPU: {}\nNPU: {}\nTier: {:?}",
            hw.cpu.model,
            hw.cpu.physical_cores,
            hw.memory.total_mb as f64 / 1024.0,
            if gpus.is_empty() {
                "none".into()
            } else {
                gpus.join(", ")
            },
            if hw.npus.is_empty() {
                "none".into()
            } else {
                hw.npus
                    .iter()
                    .map(|n| n.device.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            },
            report.recommendation.tier,
        );
        let system = format!("{}, {}", r::distro(), r::gnome_version());
        let status = s.status.lock().unwrap().clone();
        let models = format!(
            "Live: {}\nCorrection: {}\nAccelerator: {}",
            report.active_live.as_deref().unwrap_or("none"),
            report.active_refine.as_deref().unwrap_or("none"),
            captionist_engine::accel::active()
                .map(|k| k.label())
                .unwrap_or("processor"),
        );
        let errors = s.reporter.recent_errors();
        let logs = format!(
            "Status: {status}\n{models}\n\nRecent errors:\n{}\n\nService log:\n{}",
            if errors.is_empty() {
                "(none)".into()
            } else {
                errors.join("\n")
            },
            r::recent_log(40)
        );
        let version = env!("CARGO_PKG_VERSION").to_string();
        let text = format!("Captionist {version}\nSystem: {system}\n{hardware}\n\n{logs}");
        serde_json::json!({ "version": version, "system": system, "hardware": hardware, "logs": logs, "text": text }).to_string()
    }

    /// Errors caught in the GNOME Shell extension (which can't use Sentry itself).
    fn report_extension_error(&self, place: String, message: String) {
        self.shared
            .reporter
            .error("extension", &format!("{place}: {message}"));
    }

    /// Whether this build can send crash reports at all (has a Sentry DSN).
    fn crash_reports_available(&self) -> bool {
        crate::reporting::Reporter::available()
    }

    fn delete_model(&self, id: String) -> fdo::Result<()> {
        let m = self
            .shared
            .catalog
            .get(&id)
            .cloned()
            .ok_or_else(|| fdo::Error::InvalidArgs(format!("unknown model {id}")))?;
        if m.calibration {
            return Err(fdo::Error::Failed(
                "the calibration model is needed for speed estimates".into(),
            ));
        }
        captionist_engine::download::delete(&m, &self.shared.models_root)
            .map_err(|e| fdo::Error::Failed(format!("{e:#}")))?;
        self.shared
            .measured
            .lock()
            .unwrap()
            .retain(|r| r.model_id != id);
        self.shared.models_changed();
        self.shared.send(crate::shared::Cmd::Apply);
        Ok(())
    }
}
