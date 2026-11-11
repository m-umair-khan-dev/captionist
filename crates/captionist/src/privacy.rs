//! Settings → General → "Privacy and feedback": the opt-in crash-reports
//! switch, the one-time question, and "Report a problem".

use crate::client::Client;
use adw::prelude::*;
use gtk::{gio, glib};

const ISSUES_URL: &str = "https://github.com/m-umair-khan-dev/captionist/issues/new";
const SENT: &str = "Error type and stack trace, Captionist version, distribution, GNOME version, CPU core count, RAM size, accelerator in use.";
const NEVER: &str =
    "Captions, audio, history, vocabulary, file names, your user or computer name, IP address.";

/// Percent-encodes text for a URL query value.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Keeps the end of long text (the newest log lines matter most).
fn tail(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut start = s.len() - max;
    while !s.is_char_boundary(start) {
        start += 1;
    }
    format!("…{}", &s[start..])
}

pub fn group(
    settings: &gio::Settings,
    client: &Client,
    toast: &adw::ToastOverlay,
) -> adw::PreferencesGroup {
    let g = adw::PreferencesGroup::builder()
        .title("Privacy and feedback")
        .description("Captions and audio never leave this computer.")
        .build();

    // Crash reports switch ("ask" counts as off).
    let sw = adw::SwitchRow::builder()
        .title("Send crash reports")
        .subtitle("Anonymous error reports that help fix bugs. Off unless you turn it on.")
        .active(settings.string("crash-reports") == "on")
        .build();
    {
        let s = settings.clone();
        sw.connect_active_notify(move |w| {
            let v = if w.is_active() { "on" } else { "off" };
            if s.string("crash-reports") != v {
                let _ = s.set_string("crash-reports", v);
            }
        });
        let w = sw.clone();
        settings.connect_changed(Some("crash-reports"), move |s, _| {
            let on = s.string("crash-reports") == "on";
            if w.is_active() != on {
                w.set_active(on);
            }
        });
    }
    {
        // Builds without a Sentry address can't send anything: say so.
        let (sw, c) = (sw.clone(), client.clone());
        glib::spawn_future_local(async move {
            let available = c
                .call("CrashReportsAvailable", None)
                .await
                .ok()
                .and_then(|v| v.get::<(bool,)>())
                .is_some_and(|t| t.0);
            if !available {
                sw.set_sensitive(false);
                sw.set_subtitle("Not available in this build (no report address configured).");
            }
        });
    }
    g.add(&sw);

    let what = adw::ExpanderRow::builder()
        .title("What's in a crash report")
        .build();
    what.add_row(
        &adw::ActionRow::builder()
            .title("Sent")
            .subtitle(SENT)
            .subtitle_lines(0)
            .build(),
    );
    what.add_row(
        &adw::ActionRow::builder()
            .title("Never sent")
            .subtitle(NEVER)
            .subtitle_lines(0)
            .build(),
    );
    g.add(&what);

    // Report a problem: the user sees everything, then decides.
    let report = adw::ActionRow::builder()
        .title("Report a problem")
        .subtitle("Review a diagnostics summary, then copy it or open a pre-filled bug report")
        .activatable(true)
        .build();
    report.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    {
        let (c, t) = (client.clone(), toast.clone());
        report.connect_activated(move |row| report_dialog(row, &c, &t));
    }
    g.add(&report);
    g
}

fn report_dialog(anchor: &adw::ActionRow, client: &Client, toast: &adw::ToastOverlay) {
    let (anchor, client, toast) = (anchor.clone(), client.clone(), toast.clone());
    glib::spawn_future_local(async move {
        let json = match client.call_string("DiagnosticsReport", None).await {
            Ok(j) => j,
            Err(e) => {
                toast.add_toast(adw::Toast::new(&format!(
                    "Couldn't collect diagnostics: {}",
                    e.message()
                )));
                return;
            }
        };
        let v: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
        let field = |k: &str| v[k].as_str().unwrap_or("").to_string();
        let text = field("text");

        let view = gtk::TextView::builder()
            .editable(false)
            .monospace(true)
            .wrap_mode(gtk::WrapMode::WordChar)
            .top_margin(8)
            .bottom_margin(8)
            .left_margin(8)
            .right_margin(8)
            .build();
        view.buffer().set_text(&text);
        let scroll = gtk::ScrolledWindow::builder()
            .child(&view)
            .min_content_height(320)
            .min_content_width(560)
            .css_classes(["card"])
            .build();

        let dialog = adw::AlertDialog::builder()
            .heading("Report a problem")
            .body("This is everything the report contains. User and computer names are removed, and captions are never included. Nothing is sent unless you post it.")
            .build();
        dialog.set_extra_child(Some(&scroll));
        dialog.add_responses(&[
            ("close", "Close"),
            ("copy", "Copy"),
            ("issue", "Open bug report"),
        ]);
        dialog.set_response_appearance("issue", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("issue"));
        dialog.set_close_response("close");

        let window = anchor.root().and_downcast::<gtk::Window>();
        let url = format!(
            "{ISSUES_URL}?template=bug_report.yml&version={}&system={}&hardware={}&logs={}",
            encode(&field("version")),
            encode(&field("system")),
            encode(&field("hardware")),
            encode(&tail(&field("logs"), 3500)),
        );
        let (anchor2, toast2) = (anchor.clone(), toast.clone());
        dialog.connect_response(None, move |_, resp| match resp {
            "copy" => {
                anchor2.clipboard().set_text(&text);
                toast2.add_toast(adw::Toast::new("Diagnostics copied"));
            }
            "issue" => {
                gtk::UriLauncher::new(&url).launch(window.as_ref(), gio::Cancellable::NONE, |_| {});
            }
            _ => {}
        });
        dialog.present(anchor.root().as_ref());
    });
}

/// Asks once whether to send crash reports (setting "ask"), only in builds
/// that can send them.
pub fn ask_once(settings: &gio::Settings, client: &Client, window: &adw::ApplicationWindow) {
    if settings.string("crash-reports") != "ask" {
        return;
    }
    let (settings, client, window) = (settings.clone(), client.clone(), window.clone());
    glib::spawn_future_local(async move {
        let available = client
            .call("CrashReportsAvailable", None)
            .await
            .ok()
            .and_then(|v| v.get::<(bool,)>())
            .is_some_and(|t| t.0);
        if !available {
            return;
        }
        let dialog = adw::AlertDialog::builder()
            .heading("Help improve Captionist?")
            .body(format!(
                "Captionist can send an anonymous report when something goes wrong, so bugs get fixed faster.\n\nSent: {SENT}\n\nNever sent: {NEVER}\n\nYou can change this any time in Settings → General."
            ))
            .build();
        dialog.add_responses(&[("off", "Don't send"), ("on", "Send crash reports")]);
        dialog.set_response_appearance("on", adw::ResponseAppearance::Suggested);
        dialog.set_close_response("off");
        let s = settings.clone();
        dialog.connect_response(None, move |_, resp| {
            let _ = s.set_string("crash-reports", if resp == "on" { "on" } else { "off" });
        });
        dialog.present(Some(&window));
    });
}
