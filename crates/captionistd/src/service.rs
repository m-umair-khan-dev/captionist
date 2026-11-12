//! D-Bus interface `io.github.m_umair_khan_dev.Captionist.Daemon1` on `io.github.m_umair_khan_dev.Captionist.Daemon`,
//! served with GIO's D-Bus API. Method calls arrive on the main thread; slow ones
//! run on a worker thread. Signals are emitted from any thread via `Shared::emit`.

use crate::shared::{IFACE, NAME, PATH, Shared};
use captionist_core::listing::SortBy;
use glib::variant::{FromVariant, ToVariant};
use std::rc::Rc;
use std::sync::Arc;
use std::thread;

const INTROSPECTION: &str = r#"
<node>
  <interface name="io.github.m_umair_khan_dev.Captionist.Daemon1">
    <method name="Ping"><arg name="version" type="s" direction="out"/></method>
    <method name="Status"><arg name="status" type="s" direction="out"/></method>
    <method name="ListModels">
      <arg name="sort" type="s" direction="in"/>
      <arg name="report" type="s" direction="out"/>
    </method>
    <method name="Recheck"/>
    <method name="DownloadModel"><arg name="id" type="s" direction="in"/></method>
    <method name="DownloadRecommended"/>
    <method name="InstallAccelerator"><arg name="kind" type="s" direction="in"/></method>
    <method name="RemoveAccelerator"><arg name="pack_id" type="s" direction="in"/></method>
    <method name="ListApps"><arg name="apps" type="s" direction="out"/></method>
    <method name="GetHistory"><arg name="entries" type="s" direction="out"/></method>
    <method name="ClearHistory"/>
    <method name="DiagnosticsReport"><arg name="report" type="s" direction="out"/></method>
    <method name="ReportExtensionError">
      <arg name="place" type="s" direction="in"/>
      <arg name="message" type="s" direction="in"/>
    </method>
    <method name="CrashReportsAvailable"><arg name="available" type="b" direction="out"/></method>
    <method name="DeleteModel"><arg name="id" type="s" direction="in"/></method>
    <signal name="Partial"><arg name="id" type="u"/><arg name="text" type="s"/></signal>
    <signal name="Final"><arg name="id" type="u"/><arg name="text" type="s"/></signal>
    <signal name="StatusChanged"><arg name="status" type="s"/></signal>
    <signal name="DownloadProgress"><arg name="id" type="s"/><arg name="fraction" type="d"/></signal>
    <signal name="ModelsChanged"/>
    <signal name="HistoryChanged"/>
    <signal name="Error"><arg name="message" type="s"/></signal>
  </interface>
</node>
"#;

type Reply = Result<Option<glib::Variant>, glib::Error>;

fn invalid(msg: String) -> glib::Error {
    glib::Error::new(gio::DBusError::InvalidArgs, &msg)
}

fn failed(msg: impl AsRef<str>) -> glib::Error {
    glib::Error::new(gio::DBusError::Failed, msg.as_ref())
}

fn args<T: FromVariant>(params: &glib::Variant) -> Result<T, glib::Error> {
    params
        .get::<T>()
        .ok_or_else(|| invalid(format!("unexpected arguments {}", params.type_())))
}

fn ok<T: ToVariant>(v: T) -> Reply {
    Ok(Some(v.to_variant()))
}

/// Exports the service object on `conn`, then claims the well-known name.
/// Fails if another captionistd already owns the name.
pub fn serve(conn: &gio::DBusConnection, shared: Arc<Shared>) -> anyhow::Result<()> {
    let node = gio::DBusNodeInfo::for_xml(INTROSPECTION)?;
    let iface = node
        .lookup_interface(IFACE)
        .ok_or_else(|| anyhow::anyhow!("interface missing from introspection data"))?;
    let service = Rc::new(Service { shared });
    conn.register_object(PATH, &iface)
        .method_call(move |_, _, _, _, method, params, invocation| {
            service.dispatch(method, &params, invocation)
        })
        .build()?;
    // DO_NOT_QUEUE (4): fail instead of waiting for the current owner to leave.
    let reply = conn.call_sync(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "RequestName",
        Some(&(NAME, 4u32).to_variant()),
        Some(glib::VariantTy::new("(u)").unwrap()),
        gio::DBusCallFlags::NONE,
        -1,
        gio::Cancellable::NONE,
    )?;
    // 1 = became the owner, 4 = already the owner.
    match reply.get::<(u32,)>() {
        Some((1 | 4,)) => Ok(()),
        _ => anyhow::bail!("{NAME} is already taken"),
    }
}

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

impl Service {
    fn dispatch(
        &self,
        method: &str,
        params: &glib::Variant,
        invocation: gio::DBusMethodInvocation,
    ) {
        // These run external tools (pw-dump, journalctl): keep them off the main thread.
        let slow: Option<Box<dyn FnOnce() -> String + Send>> = match method {
            "ListApps" => Some(Box::new(list_apps)),
            "DiagnosticsReport" => {
                let s = self.shared.clone();
                Some(Box::new(move || diagnostics_report(&s)))
            }
            _ => None,
        };
        if let Some(job) = slow {
            invocation.return_future_local(async move {
                gio::spawn_blocking(job)
                    .await
                    .map(|s| Some(s.to_variant()))
                    .map_err(|_| failed("internal error"))
            });
            return;
        }
        invocation.return_result(self.call(method, params));
    }

    fn call(&self, method: &str, p: &glib::Variant) -> Reply {
        match method {
            "Ping" => ok(env!("CARGO_PKG_VERSION")),
            "Status" => ok(self.status()),
            "ListModels" => ok(self.list_models(args::<(String,)>(p)?.0)),
            "Recheck" => {
                self.recheck();
                Ok(None)
            }
            "DownloadModel" => self.download_model(args::<(String,)>(p)?.0).map(|_| None),
            "DownloadRecommended" => {
                self.download_recommended();
                Ok(None)
            }
            "InstallAccelerator" => self
                .install_accelerator(args::<(String,)>(p)?.0)
                .map(|_| None),
            "RemoveAccelerator" => self
                .remove_accelerator(args::<(String,)>(p)?.0)
                .map(|_| None),
            "GetHistory" => ok(self.get_history()),
            "ClearHistory" => {
                self.clear_history();
                Ok(None)
            }
            "ReportExtensionError" => {
                let (place, message) = args::<(String, String)>(p)?;
                self.report_extension_error(place, message);
                Ok(None)
            }
            "CrashReportsAvailable" => ok(crate::reporting::Reporter::available()),
            "DeleteModel" => self.delete_model(args::<(String,)>(p)?.0).map(|_| None),
            _ => Err(glib::Error::new(
                gio::DBusError::UnknownMethod,
                &format!("unknown method {method}"),
            )),
        }
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

    fn download_model(&self, id: String) -> Result<(), glib::Error> {
        let m = self
            .shared
            .catalog
            .get(&id)
            .cloned()
            .ok_or_else(|| invalid(format!("unknown model {id}")))?;
        spawn_job(&self.shared, "Download", move |s| {
            s.ensure_downloaded(&m)?;
            s.measure(&m)?;
            s.send(crate::shared::Cmd::Apply);
            s.test_new_models_on_accel();
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
            s.test_new_models_on_accel();
            Ok(())
        });
    }

    /// Downloads the runtime pack for an accelerator ("intel-gpu", "intel-npu",
    /// "nvidia-gpu"). Progress arrives as DownloadProgress("pack:<id>", fraction).
    fn install_accelerator(&self, kind: String) -> Result<(), glib::Error> {
        let kind = captionist_core::accel::Kind::parse(&kind)
            .ok_or_else(|| invalid(format!("unknown accelerator {kind}")))?;
        let hw = self.shared.hw.lock().unwrap().clone();
        let pack = captionist_core::accel::pack_for(kind, &hw)
            .ok_or_else(|| failed("No runtime pack fits this PC's drivers yet"))?;
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

    fn remove_accelerator(&self, pack_id: String) -> Result<(), glib::Error> {
        let hw = self.shared.hw.lock().unwrap().clone();
        if let Some(k) = captionist_engine::accel::active()
            && captionist_core::accel::pack_for(k, &hw).is_some_and(|p| p.id == pack_id)
        {
            return Err(failed("Turn this accelerator off first"));
        }
        captionist_engine::accel::remove_pack(&pack_id).map_err(|e| failed(format!("{e:#}")))?;
        self.shared.models_changed();
        Ok(())
    }

    /// JSON list of caption history entries: [{at, duration, text}]
    fn get_history(&self) -> String {
        serde_json::to_string(&self.shared.history.lock().unwrap().entries)
            .unwrap_or_else(|_| "[]".to_string())
    }

    fn clear_history(&self) {
        self.shared.history.lock().unwrap().clear();
        self.shared.emit("HistoryChanged", &());
    }

    /// Errors caught in the GNOME Shell extension (which can't use Sentry itself).
    fn report_extension_error(&self, place: String, message: String) {
        self.shared
            .reporter
            .error("extension", &format!("{place}: {message}"));
    }

    fn delete_model(&self, id: String) -> Result<(), glib::Error> {
        let m = self
            .shared
            .catalog
            .get(&id)
            .cloned()
            .ok_or_else(|| invalid(format!("unknown model {id}")))?;
        if m.calibration {
            return Err(failed(
                "the calibration model is needed for speed estimates",
            ));
        }
        captionist_engine::download::delete(&m, &self.shared.models_root)
            .map_err(|e| failed(format!("{e:#}")))?;
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

/// JSON list of apps playing audio right now: [{name, media, icon}]
fn list_apps() -> String {
    serde_json::to_string(&captionist_engine::capture::list_audio_apps())
        .unwrap_or_else(|_| "[]".to_string())
}

/// JSON for "Report a problem": {version, system, hardware, logs, text}.
/// Scrubbed of user and host names; never contains caption text.
fn diagnostics_report(s: &Shared) -> String {
    use crate::reporting as r;
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
