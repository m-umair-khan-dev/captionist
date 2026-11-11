//! Settings → Accuracy: custom vocabulary and replacements editors.

use adw::prelude::*;
use gtk::gio;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Vocabulary list: an entry row to add words, then one row per word.
pub fn vocabulary_group(settings: &gio::Settings) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Custom vocabulary")
        .description(
            "Names, products and jargon. Captions that sound like one of these are written \
             exactly as you typed it, e.g. \"cooper netties\" → \"Kubernetes\".",
        )
        .build();
    let add = adw::EntryRow::builder()
        .title("Add a word or name")
        .show_apply_button(true)
        .build();
    group.add(&add);

    let rows: Rc<RefCell<Vec<adw::ActionRow>>> = Rc::default();
    let rebuild = {
        let (group, settings, rows) = (group.clone(), settings.clone(), rows.clone());
        Rc::new(move || {
            for r in rows.borrow_mut().drain(..) {
                group.remove(&r);
            }
            for word in settings.strv("vocabulary").iter() {
                let word = word.to_string();
                let row = adw::ActionRow::builder()
                    .title(&word)
                    .use_markup(false)
                    .build();
                let del = gtk::Button::builder()
                    .icon_name("user-trash-symbolic")
                    .valign(gtk::Align::Center)
                    .css_classes(["flat"])
                    .tooltip_text("Remove")
                    .build();
                let s = settings.clone();
                del.connect_clicked(move |_| {
                    let list: Vec<String> = s
                        .strv("vocabulary")
                        .iter()
                        .map(|v| v.to_string())
                        .filter(|v| *v != word)
                        .collect();
                    let _ = s.set_strv(
                        "vocabulary",
                        list.iter().map(String::as_str).collect::<Vec<_>>(),
                    );
                });
                row.add_suffix(&del);
                group.add(&row);
                rows.borrow_mut().push(row);
            }
        })
    };
    rebuild();
    {
        let rebuild = rebuild.clone();
        settings.connect_changed(Some("vocabulary"), move |_, _| rebuild());
    }
    let s = settings.clone();
    add.connect_apply(move |e| {
        let text = e.text().trim().to_string();
        if text.is_empty() {
            return;
        }
        let mut list: Vec<String> = s.strv("vocabulary").iter().map(|v| v.to_string()).collect();
        if !list.iter().any(|v| v.eq_ignore_ascii_case(&text)) {
            list.push(text);
            let _ = s.set_strv(
                "vocabulary",
                list.iter().map(String::as_str).collect::<Vec<_>>(),
            );
        }
        e.set_text("");
    });
    group
}

fn replacements(s: &gio::Settings) -> HashMap<String, String> {
    s.value("replacements")
        .get::<HashMap<String, String>>()
        .unwrap_or_default()
}

/// Replacements: "when you hear X, write Y".
pub fn replacements_group(settings: &gio::Settings) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Replacements")
        .description("Always write these words differently, e.g. \"pipe wire\" → \"PipeWire\".")
        .build();

    let add_row = adw::ActionRow::new();
    let heard = gtk::Entry::builder()
        .placeholder_text("Heard")
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    let write = gtk::Entry::builder()
        .placeholder_text("Write instead")
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    let arrow = gtk::Image::from_icon_name("go-next-symbolic");
    let add = gtk::Button::builder()
        .icon_name("list-add-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text("Add")
        .build();
    let bx = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    bx.set_margin_top(6);
    bx.set_margin_bottom(6);
    bx.append(&heard);
    bx.append(&arrow);
    bx.append(&write);
    bx.append(&add);
    add_row.set_child(Some(&bx));
    group.add(&add_row);

    let do_add = {
        let (s, heard, write) = (settings.clone(), heard.clone(), write.clone());
        move || {
            let (h, w) = (
                heard.text().trim().to_string(),
                write.text().trim().to_string(),
            );
            if h.is_empty() || w.is_empty() {
                return;
            }
            let mut map = replacements(&s);
            map.insert(h, w);
            let _ = s.set_value("replacements", &map.to_variant());
            heard.set_text("");
            write.set_text("");
            heard.grab_focus();
        }
    };
    {
        let f = do_add.clone();
        add.connect_clicked(move |_| f());
    }
    {
        let f = do_add.clone();
        write.connect_activate(move |_| f());
    }

    let rows: Rc<RefCell<Vec<adw::ActionRow>>> = Rc::default();
    let rebuild = {
        let (group, settings, rows) = (group.clone(), settings.clone(), rows.clone());
        Rc::new(move || {
            for r in rows.borrow_mut().drain(..) {
                group.remove(&r);
            }
            let mut items: Vec<(String, String)> = replacements(&settings).into_iter().collect();
            items.sort();
            for (from, to) in items {
                let row = adw::ActionRow::builder()
                    .title(format!("{from}  →  {to}"))
                    .use_markup(false)
                    .build();
                let del = gtk::Button::builder()
                    .icon_name("user-trash-symbolic")
                    .valign(gtk::Align::Center)
                    .css_classes(["flat"])
                    .tooltip_text("Remove")
                    .build();
                let s = settings.clone();
                del.connect_clicked(move |_| {
                    let mut map = replacements(&s);
                    map.remove(&from);
                    let _ = s.set_value("replacements", &map.to_variant());
                });
                row.add_suffix(&del);
                group.add(&row);
                rows.borrow_mut().push(row);
            }
        })
    };
    rebuild();
    settings.connect_changed(Some("replacements"), move |_, _| rebuild());
    group
}
