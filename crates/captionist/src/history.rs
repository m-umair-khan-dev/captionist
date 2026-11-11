//! Settings → History: recent captions, copy, export (.txt/.srt/.vtt), clear.

use crate::client::Client;
use crate::widgets::{choice_row, spin_row};
use adw::prelude::*;
use captionist_core::history::{self, Entry, Format};
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const MODES: &[(&str, &str)] = &[
    ("off", "Off"),
    ("session", "This session only"),
    ("keep", "Keep on this computer"),
];
/// Rows shown in the window (the export always includes everything).
const SHOW: usize = 300;

pub struct HistoryPage {
    pub page: adw::PreferencesPage,
    client: Client,
    toast: adw::ToastOverlay,
    list: adw::PreferencesGroup,
    rows: RefCell<Vec<gtk::Widget>>,
    entries: RefCell<Vec<Entry>>,
    refresh_pending: Cell<bool>,
}

fn utc_offset() -> i64 {
    glib::DateTime::now_local()
        .map(|d| d.utc_offset().as_seconds())
        .unwrap_or(0)
}

impl HistoryPage {
    pub fn new(client: Client, settings: gio::Settings, toast: adw::ToastOverlay) -> Rc<Self> {
        let page = adw::PreferencesPage::new();

        let opts = adw::PreferencesGroup::builder()
            .title("Keep history")
            .description(
                "Saved history stays on this computer, in ~/.local/state/captionist/history.",
            )
            .build();
        opts.add(&choice_row(&settings, "history-mode", "History", "", MODES));
        let days = spin_row(
            &settings,
            "history-days",
            "Keep for",
            "Days, when kept on this computer",
            1.0,
            365.0,
            1.0,
        );
        let sync_days = {
            let days = days.clone();
            move |s: &gio::Settings| days.set_sensitive(s.string("history-mode") == "keep")
        };
        sync_days(&settings);
        settings.connect_changed(Some("history-mode"), move |s, _| sync_days(s));
        opts.add(&days);
        page.add(&opts);

        let list = adw::PreferencesGroup::builder().title("Captions").build();
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let copy = gtk::Button::builder()
            .icon_name("edit-copy-symbolic")
            .tooltip_text("Copy all")
            .css_classes(["flat"])
            .build();
        let export = gtk::Button::builder()
            .label("Export…")
            .css_classes(["flat"])
            .build();
        let clear = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .tooltip_text("Clear history")
            .css_classes(["flat"])
            .build();
        buttons.append(&copy);
        buttons.append(&export);
        buttons.append(&clear);
        list.set_header_suffix(Some(&buttons));
        page.add(&list);

        let me = Rc::new(HistoryPage {
            page,
            client,
            toast,
            list,
            rows: RefCell::new(vec![]),
            entries: RefCell::new(vec![]),
            refresh_pending: Cell::new(false),
        });

        let m = me.clone();
        copy.connect_clicked(move |b| {
            let text = history::export(&m.entries.borrow(), Format::Text, utc_offset());
            b.clipboard().set_text(&text);
            m.toast.add_toast(adw::Toast::new("Captions copied"));
        });
        let m = me.clone();
        export.connect_clicked(move |b| m.export(b));
        let m = me.clone();
        clear.connect_clicked(move |b| {
            let dialog = adw::AlertDialog::builder()
                .heading("Clear caption history?")
                .body("This removes all saved captions, including any kept on disk.")
                .build();
            dialog.add_responses(&[("cancel", "Cancel"), ("clear", "Clear")]);
            dialog.set_response_appearance("clear", adw::ResponseAppearance::Destructive);
            let m2 = m.clone();
            dialog.connect_response(None, move |_, r| {
                if r == "clear" {
                    m2.client.spawn_call("ClearHistory", None, |_| {});
                }
            });
            dialog.present(b.root().as_ref());
        });
        me
    }

    pub fn on_signal(self: &Rc<Self>, name: &str) {
        if matches!(name, "Final" | "HistoryChanged") && !self.refresh_pending.replace(true) {
            // Batch bursts of captions into one refresh.
            let me = self.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(600), move || {
                me.refresh_pending.set(false);
                me.refresh();
            });
        }
    }

    pub fn refresh(self: &Rc<Self>) {
        let me = self.clone();
        glib::spawn_future_local(async move {
            if let Ok(json) = me.client.call_string("GetHistory", None).await {
                let entries: Vec<Entry> = serde_json::from_str(&json).unwrap_or_default();
                me.rebuild(entries);
            }
        });
    }

    fn rebuild(&self, entries: Vec<Entry>) {
        for w in self.rows.borrow_mut().drain(..) {
            self.list.remove(&w);
        }
        let off = utc_offset();
        if entries.is_empty() {
            let row = adw::ActionRow::builder()
                .title("No captions yet")
                .subtitle("Captions appear here as they're finalised")
                .build();
            self.list.add(&row);
            self.rows.borrow_mut().push(row.upcast());
        }
        // Newest first.
        for e in entries.iter().rev().take(SHOW) {
            let row = adw::ActionRow::builder()
                .title(&e.text)
                .subtitle(history::clock(e.at, off))
                .use_markup(false)
                .title_lines(0)
                .css_classes(["property"])
                .build();
            self.list.add(&row);
            self.rows.borrow_mut().push(row.upcast());
        }
        self.list
            .set_description(Some(&format!("{} captions", entries.len())));
        *self.entries.borrow_mut() = entries;
    }

    fn export(self: &Rc<Self>, anchor: &gtk::Button) {
        let day = glib::DateTime::now_local()
            .ok()
            .and_then(|d| d.format("%Y-%m-%d").ok())
            .map(|s| s.to_string())
            .unwrap_or_default();
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        for (name, pat) in [
            ("Subtitles (.srt)", "*.srt"),
            ("Web subtitles (.vtt)", "*.vtt"),
            ("Text (.txt)", "*.txt"),
        ] {
            let f = gtk::FileFilter::new();
            f.set_name(Some(name));
            f.add_pattern(pat);
            filters.append(&f);
        }
        let dialog = gtk::FileDialog::builder()
            .title("Export captions")
            .initial_name(format!("captions-{day}.srt"))
            .filters(&filters)
            .build();
        let window = anchor.root().and_downcast::<gtk::Window>();
        let me = self.clone();
        dialog.save(window.as_ref(), gio::Cancellable::NONE, move |res| {
            let Ok(file) = res else { return };
            let Some(path) = file.path() else { return };
            let p = path.to_string_lossy().to_string();
            let text = history::export(&me.entries.borrow(), Format::from_path(&p), utc_offset());
            match std::fs::write(&path, text) {
                Ok(()) => me.toast.add_toast(adw::Toast::new(&format!(
                    "Saved {}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                ))),
                Err(e) => me
                    .toast
                    .add_toast(adw::Toast::new(&format!("Could not save: {e}"))),
            }
        });
    }
}
