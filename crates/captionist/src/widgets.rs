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

pub fn spin_row(
    settings: &gio::Settings,
    key: &str,
    title: &str,
    subtitle: &str,
    min: f64,
    max: f64,
    step: f64,
) -> adw::SpinRow {
    let row = adw::SpinRow::with_range(min, max, step);
    row.set_title(title);
    row.set_subtitle(subtitle);
    row.set_value(settings.int(key) as f64);
    let k = key.to_string();
    let s = settings.clone();
    row.connect_value_notify(move |r| {
        let v = r.value().round() as i32;
        if s.int(&k) != v {
            let _ = s.set_int(&k, v);
        }
    });
    let r2 = row.clone();
    settings.connect_changed(Some(key), move |s, k| {
        let v = s.int(k) as f64;
        if (r2.value() - v).abs() > 0.1 {
            r2.set_value(v);
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
