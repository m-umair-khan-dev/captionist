//! Problem reporting.
//!
//! Two layers, both under the user's control (see PRIVACY.md):
//! 1. A diagnostics report the user reviews and posts themselves
//!    ("Report a problem" in Settings). Nothing is sent by Captionist.
//! 2. Optional crash and error reports to Sentry, only when the
//!    `crash-reports` setting is "on" and the build has a DSN.
//!
//! Never included: caption text, audio, history, vocabulary, file names,
//! user or host names, IP address (dropped server-side).

use std::borrow::Cow;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};

/// Sentry project address. Injected at build time (CI variable), so local and
/// forked builds never report anywhere.
pub const SENTRY_DSN: Option<&str> = option_env!("CAPTIONIST_SENTRY_DSN");

/// Recent errors, kept in memory for the diagnostics report.
const MAX_RECENT: usize = 20;

pub struct Reporter {
    guard: Mutex<Option<sentry::ClientInitGuard>>,
    recent: Mutex<VecDeque<String>>,
}

/// Removes things that could identify the user from text.
pub fn scrub(text: &str) -> String {
    // Longest first: the computer name often contains the user name
    // (e.g. "alex-laptop"), and the home path contains it too.
    let mut s = text.to_string();
    if let Some(home) = std::env::var_os("HOME").map(|h| h.to_string_lossy().to_string())
        && home.len() > 1
    {
        s = s.replace(&home, "~");
    }
    if let Some(host) = hostname()
        && host.len() > 2
    {
        s = s.replace(&host, "<host>");
    }
    for var in ["USER", "LOGNAME"] {
        if let Ok(u) = std::env::var(var)
            && u.len() > 2
        {
            s = s.replace(&u, "<user>");
        }
    }
    s
}

fn hostname() -> Option<String> {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
}

fn scrub_event(
    mut ev: sentry::protocol::Event<'static>,
) -> Option<sentry::protocol::Event<'static>> {
    ev.server_name = None;
    ev.user = None;
    ev.request = None;
    ev.breadcrumbs = Default::default();
    if let Some(m) = ev.message.take() {
        ev.message = Some(scrub(&m));
    }
    for ex in ev.exception.values.iter_mut() {
        if let Some(v) = ex.value.take() {
            ex.value = Some(scrub(&v));
        }
        if let Some(st) = ex.stacktrace.as_mut() {
            for f in st.frames.iter_mut() {
                if let Some(p) = f.abs_path.take() {
                    f.abs_path = Some(scrub(&p));
                }
                f.vars.clear();
            }
        }
    }
    for v in ev.extra.values_mut() {
        if let Some(s) = v.as_str() {
            *v = scrub(s).into();
        }
    }
    Some(ev)
}

impl Reporter {
    pub fn new() -> Self {
        Reporter {
            guard: Mutex::new(None),
            recent: Mutex::new(VecDeque::new()),
        }
    }

    pub fn available() -> bool {
        SENTRY_DSN.is_some_and(|d| !d.is_empty())
    }

    pub fn is_on(&self) -> bool {
        self.guard.lock().unwrap().is_some()
    }

    /// Turns automatic reports on or off (the `crash-reports` setting).
    pub fn set_enabled(&self, on: bool, tags: &[(&str, String)]) {
        let mut guard = self.guard.lock().unwrap();
        match (on, guard.is_some()) {
            (true, false) => {
                let Some(dsn) = SENTRY_DSN.filter(|d| !d.is_empty()) else {
                    return;
                };
                let mut opts = sentry::ClientOptions::default();
                opts.release = Some(Cow::Owned(format!(
                    "captionist@{}",
                    env!("CARGO_PKG_VERSION")
                )));
                opts.environment = Some(Cow::Borrowed("production"));
                opts.send_default_pii = false; // no IPs or user data, ever
                opts.server_name = None; // not the computer's name
                opts.attach_stacktrace = true;
                opts.auto_session_tracking = false; // no usage tracking
                opts.before_send = Some(Arc::new(scrub_event));
                opts.before_breadcrumb = Some(Arc::new(|_| None));
                let g = sentry::init((dsn, opts));
                sentry::configure_scope(|scope| {
                    for (k, v) in tags {
                        scope.set_tag(k, v);
                    }
                });
                *guard = Some(g);
                eprintln!("crash reports: on");
            }
            (false, true) => {
                drop(guard.take()); // flushes pending reports
                sentry::Hub::current().bind_client(None);
                eprintln!("crash reports: off");
            }
            _ => {}
        }
    }

    /// Records an error: kept for the diagnostics report, and sent to Sentry
    /// when the user turned crash reports on. `kind` is a short category
    /// ("pipeline", "download", "accelerator", "extension", "crash").
    pub fn error(&self, kind: &str, message: &str) {
        let msg = scrub(message);
        {
            let mut r = self.recent.lock().unwrap();
            let stamp = glib::DateTime::now_local()
                .ok()
                .and_then(|d| d.format("%H:%M:%S").ok())
                .map(|s| s.to_string())
                .unwrap_or_default();
            r.push_back(format!("{stamp} [{kind}] {msg}"));
            while r.len() > MAX_RECENT {
                r.pop_front();
            }
        }
        if self.is_on() {
            sentry::with_scope(
                |scope| scope.set_tag("kind", kind),
                || sentry::capture_message(&msg, sentry::Level::Error),
            );
        }
    }

    pub fn recent_errors(&self) -> Vec<String> {
        self.recent.lock().unwrap().iter().cloned().collect()
    }
}

// ── Unexpected exits ───────────────────────────────────────────────────────
// The speech engine is C++: a crash there kills the process without a Rust
// panic. A marker file exists while the service runs and is removed on a
// clean exit; finding it at startup means the last run ended unexpectedly.

fn marker() -> PathBuf {
    captionist_engine::paths::state_dir().join("captionistd.running")
}

/// Returns true if the previous run ended unexpectedly; then marks this run.
pub fn check_and_mark_running() -> bool {
    let m = marker();
    let crashed = m.exists();
    let _ = std::fs::create_dir_all(captionist_engine::paths::state_dir());
    let _ = std::fs::write(&m, std::process::id().to_string());
    crashed
}

pub fn mark_clean_exit() {
    let _ = std::fs::remove_file(marker());
}

/// Last lines of the caption service's own log (scrubbed).
pub fn recent_log(lines: usize) -> String {
    let out = Command::new("journalctl")
        .args([
            "--user",
            "-u",
            "captionistd",
            "-b",
            "--no-pager",
            "-o",
            "cat", // messages only: no hostname or user columns
            "-n",
            &lines.to_string(),
        ])
        .output();
    match out {
        Ok(o) => scrub(String::from_utf8_lossy(&o.stdout).trim()),
        Err(_) => "(journal not available)".into(),
    }
}

fn first_line(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string()
        })
        .unwrap_or_default()
}

pub fn distro() -> String {
    std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("PRETTY_NAME="))
                .map(|l| l[12..].trim_matches('"').to_string())
        })
        .unwrap_or_else(|| "unknown".into())
}

pub fn gnome_version() -> String {
    let v = first_line("gnome-shell", &["--version"]);
    if v.is_empty() { "unknown".into() } else { v }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrub_removes_user_home_and_host() {
        let home = std::env::var("HOME").unwrap_or_default();
        let user = std::env::var("USER").unwrap_or_default();
        let text =
            format!("failed to open {home}/.local/share/captionist/models/x.onnx for {user}");
        let out = scrub(&text);
        if home.len() > 1 {
            assert!(!out.contains(&home), "home path leaked: {out}");
            assert!(out.contains("~/.local/share/captionist"));
        }
        if user.len() > 2 {
            assert!(!out.contains(&user), "user name leaked: {out}");
        }
        if let Some(h) = hostname().filter(|h| h.len() > 2) {
            let out = scrub(&format!("on {h}"));
            assert!(
                !out.contains(&h) && out.contains("<host>"),
                "host name leaked: {out}"
            );
        }
    }
}
