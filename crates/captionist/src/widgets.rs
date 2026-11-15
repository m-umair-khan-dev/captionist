//! Small helpers binding GSettings keys to libadwaita rows.

use adw::prelude::*;
use gtk::gio;

pub fn switch_row(
    settings: &gio::Settings,
    key: &str,
    title: &str,
    subtitle: &str,
) -> adw::SwitchRow {
    let row = adw::SwitchRow::builder()
        .title(title)
        .subtitle(subtitle)
        .build();
    settings.bind(key, &row, "active").build();
    row
}

/// Number row for an integer key.
///
/// Built as an `ActionRow` with a `SpinButton` suffix rather than an
/// `adw::SpinRow`: libadwaita 1.10 leaves `SpinRow` out of the accessibility
/// tree entirely, so screen readers (and UI tests) can't reach it. The spin
/// button is labelled by the row's title.
pub fn spin_row(
    settings: &gio::Settings,
    key: &str,
    title: &str,
    subtitle: &str,
    min: f64,
    max: f64,
    step: f64,
) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .build();
    let spin = gtk::SpinButton::with_range(min, max, step);
    spin.set_valign(gtk::Align::Center);
    spin.set_numeric(true);
    spin.set_value(settings.int(key) as f64);
    spin.update_property(&[gtk::accessible::Property::Label(title)]);
    row.add_suffix(&spin);
    row.set_activatable_widget(Some(&spin));
    // A greyed-out row must grey out its number too: otherwise assistive tech
    // still reports it enabled and can change it.
    row.bind_property("sensitive", &spin, "sensitive")
        .sync_create()
        .build();

    let k = key.to_string();
    let s = settings.clone();
    spin.connect_value_changed(move |b| {
        let v = b.value().round() as i32;
        // GTK still accepts values from assistive tech while greyed out.
        if !b.is_sensitive() {
            if s.int(&k) != v {
                b.set_value(s.int(&k) as f64);
            }
            return;
        }
        if s.int(&k) != v {
            let _ = s.set_int(&k, v);
        }
    });
    settings.connect_changed(Some(key), move |s, k| {
        let v = s.int(k) as f64;
        if (spin.value() - v).abs() > 0.1 {
            spin.set_value(v);
        }
    });
    row
}

/// Combo row for a string key with fixed choices: `(value, label)`.
pub fn choice_row(
    settings: &gio::Settings,
    key: &str,
    title: &str,
    subtitle: &str,
    choices: &'static [(&'static str, &'static str)],
) -> adw::ComboRow {
    let labels: Vec<&str> = choices.iter().map(|c| c.1).collect();
    let row = adw::ComboRow::builder()
        .title(title)
        .subtitle(subtitle)
        .model(&gtk::StringList::new(&labels))
        .build();
    let sync = |row: &adw::ComboRow, s: &gio::Settings, key: &str| {
        let cur = s.string(key);
        if let Some(i) = choices.iter().position(|c| c.0 == cur.as_str())
            && row.selected() != i as u32
        {
            row.set_selected(i as u32);
        }
    };
    sync(&row, settings, key);
    let (s, k) = (settings.clone(), key.to_string());
    row.connect_selected_notify(move |r| {
        if let Some(c) = choices.get(r.selected() as usize)
            && s.string(&k) != c.0
        {
            let _ = s.set_string(&k, c.0);
        }
    });
    let r2 = row.clone();
    settings.connect_changed(Some(key), move |s, k| sync(&r2, s, k));
    row
}

/// Colour picker for a "#rrggbb" string key.
pub fn color_row(settings: &gio::Settings, key: &str, title: &str) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title(title).build();
    let button =
        gtk::ColorDialogButton::new(Some(gtk::ColorDialog::builder().with_alpha(false).build()));
    button.set_valign(gtk::Align::Center);
    if let Ok(c) = gtk::gdk::RGBA::parse(settings.string(key).as_str()) {
        button.set_rgba(&c);
    }
    let (s, k) = (settings.clone(), key.to_string());
    button.connect_rgba_notify(move |b| {
        let c = b.rgba();
        let hex = format!(
            "#{:02x}{:02x}{:02x}",
            (c.red() * 255.0).round() as u8,
            (c.green() * 255.0).round() as u8,
            (c.blue() * 255.0).round() as u8
        );
        if s.string(&k) != hex {
            let _ = s.set_string(&k, &hex);
        }
    });
    row.add_suffix(&button);
    row.set_activatable_widget(Some(&button));
    row
}
