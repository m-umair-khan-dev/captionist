//! Captionist settings window.

mod client;
mod history;
mod models;
mod privacy;
mod vocab;
mod widgets;

use adw::prelude::*;
use client::Client;
use gtk::{gio, glib};
use widgets::{choice_row, color_row, spin_row, switch_row};

const APP_ID: &str = "io.github.m_umair_khan_dev.Captionist";

const SOURCES: &[(&str, &str)] = &[
    ("speakers", "Speakers / headphones"),
    ("mic", "Microphone"),
    ("both", "Speakers + microphone"),
    ("app", "One app only"),
];
const POSITIONS: &[(&str, &str)] = &[("bottom", "Bottom of screen"), ("top", "Top of screen")];

fn status_text(json: &str) -> (String, &'static str) {
    let v: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    let state = v["state"].as_str().unwrap_or("off");
    let msg = v["message"].as_str().unwrap_or("");
    let models = [v["live"].as_str(), v["refine"].as_str()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" + ");
    match state {
        "listening" => (format!("Listening · {models}"), "success"),
        "loading" => (format!("Loading {models}…"), "dim-label"),
        "setup-needed" => (msg.to_string(), "warning"),
        "error" => (format!("Problem: {msg}"), "error"),
        _ => ("Off".into(), "dim-label"),
    }
}

fn general_page(
    settings: &gio::Settings,
    status: &adw::ActionRow,
    window: &adw::ApplicationWindow,
) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::new();
    let g = adw::PreferencesGroup::builder()
        .title("Live captions")
        .build();
    g.add(&switch_row(
        settings,
        "enabled",
        "Live captions",
        "Super+Ctrl+L also turns them on and off",
    ));
    g.add(status);
    page.add(&g);

    let g = adw::PreferencesGroup::builder().title("Controls").build();
    g.add(&switch_row(
        settings,
        "show-indicator",
        "Top bar icon",
        "Quick menu: on/off, source, position, settings",
    ));
    g.add(&switch_row(
        settings,
        "show-quick-settings",
        "Quick Settings toggle",
        "Next to Wi-Fi and Bluetooth",
    ));
    page.add(&g);

    let g = adw::PreferencesGroup::new();
    let about = adw::ActionRow::builder()
        .title("About Captionist")
        .activatable(true)
        .build();
    about.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    let w = window.clone();
    about.connect_activated(move |_| {
        let d = adw::AboutDialog::builder()
            .application_name("Captionist")
            .application_icon(APP_ID)
            .version(env!("CARGO_PKG_VERSION"))
            .developer_name("Captionist contributors")
            .license_type(gtk::License::Gpl30)
            .comments("Real-time, private, on-device live captions for Linux. Captions and audio never leave your computer.")
            .build();
        d.add_credit_section(
            Some("Speech models and data"),
            &[
                "Kroko by Banafo (CC-BY-SA)",
                "Parakeet and Nemotron by NVIDIA",
                "Moonshine by Useful Sensors (MIT)",
                "sherpa-onnx by k2-fsa (Apache-2.0)",
                "Silero VAD (MIT)",
                "Calibration audio from LibriSpeech (CC-BY 4.0)",
            ],
        );
        d.present(Some(&w));
    });
    g.add(&about);
    page.add(&g);
    page
}

/// "One app only": pick from apps playing audio now (plus the saved one).
fn app_picker(settings: &gio::Settings, client: &Client) -> adw::ComboRow {
    let model = gtk::StringList::new(&[]);
    let row = adw::ComboRow::builder()
        .title("App")
        .subtitle(
            "Only this app's audio is captioned. Start playing something in it to see it here.",
        )
        .model(&model)
        .build();
    let refresh = gtk::Button::builder()
        .icon_name("view-refresh-symbolic")
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .tooltip_text("Refresh the list of apps playing audio")
        .build();
    row.add_suffix(&refresh);
    let names: std::rc::Rc<std::cell::RefCell<Vec<String>>> = Default::default();
    let updating = std::rc::Rc::new(std::cell::Cell::new(false));

    let load = {
        let (row, model, names, settings, client, updating) = (
            row.clone(),
            model.clone(),
            names.clone(),
            settings.clone(),
            client.clone(),
            updating.clone(),
        );
        move || {
            let (row, model, names, settings, client, updating) = (
                row.clone(),
                model.clone(),
                names.clone(),
                settings.clone(),
                client.clone(),
                updating.clone(),
            );
            glib::spawn_future_local(async move {
                let json = client
                    .call_string("ListApps", None)
                    .await
                    .unwrap_or_else(|_| "[]".into());
                let apps: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
                let mut list: Vec<(String, String)> = apps
                    .iter()
                    .filter_map(|a| {
                        let n = a["name"].as_str()?.to_string();
                        let m = a["media"].as_str().unwrap_or("");
                        Some((
                            n.clone(),
                            if m.is_empty() {
                                n
                            } else {
                                format!("{n} — {m}")
                            },
                        ))
                    })
                    .collect();
                let saved = settings.string("source-app").to_string();
                if !saved.is_empty() && !list.iter().any(|(n, _)| *n == saved) {
                    list.insert(0, (saved.clone(), format!("{saved} (not playing now)")));
                }
                updating.set(true);
                model.splice(
                    0,
                    model.n_items(),
                    &list.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>(),
                );
                *names.borrow_mut() = list.iter().map(|(n, _)| n.clone()).collect();
                if let Some(i) = names.borrow().iter().position(|n| *n == saved) {
                    row.set_selected(i as u32);
                } else if let Some(first) = names.borrow().first() {
                    let _ = settings.set_string("source-app", first);
                }
                updating.set(false);
            });
        }
    };
    load();
    {
        let load = load.clone();
        refresh.connect_clicked(move |_| load());
    }
    {
        let (names, settings, updating) = (names.clone(), settings.clone(), updating.clone());
        row.connect_selected_notify(move |r| {
            if updating.get() {
                return;
            }
            if let Some(n) = names.borrow().get(r.selected() as usize) {
                let _ = settings.set_string("source-app", n);
            }
        });
    }
    row.set_visible(settings.string("source") == "app");
    let r = row.clone();
    settings.connect_changed(Some("source"), move |s, _| {
        r.set_visible(s.string("source") == "app");
        if s.string("source") == "app" {
            load();
        }
    });
    row
}

fn audio_page(settings: &gio::Settings, client: &Client) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::new();
    let g = adw::PreferencesGroup::builder()
        .title("Caption audio from")
        .description("Speakers captions whatever you hear: videos, calls, meetings.")
        .build();
    g.add(&choice_row(settings, "source", "Source", "", SOURCES));
    g.add(&app_picker(settings, client));
    page.add(&g);
    let g = adw::PreferencesGroup::builder().title("Timing").build();
    g.add(&spin_row(
        settings,
        "hide-after",
        "Hide captions after silence",
        "Seconds; 0 keeps them on screen",
        0.0,
        120.0,
        1.0,
    ));
    page.add(&g);
    page
}

fn appearance_page(settings: &gio::Settings) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::new();
    let g = adw::PreferencesGroup::builder().title("Position").build();
    g.add(&choice_row(
        settings,
        "position",
        "Start position",
        "Drag the captions to move them anywhere; choosing a position snaps them back",
        POSITIONS,
    ));
    g.add(&spin_row(
        settings,
        "width-percent",
        "Window width",
        "Percent of the screen width",
        25.0,
        100.0,
        5.0,
    ));
    let reset = adw::ActionRow::builder()
        .title("Reset position")
        .activatable(true)
        .build();
    reset.add_suffix(&gtk::Image::from_icon_name("view-restore-symbolic"));
    let s = settings.clone();
    reset.connect_activated(move |_| {
        let _ = s.set_int("floating-x", -1);
        let _ = s.set_int("floating-y", -1);
    });
    g.add(&reset);
    page.add(&g);

    let g = adw::PreferencesGroup::builder().title("Text").build();
    g.add(&spin_row(
        settings,
        "font-size",
        "Font size",
        "Pixels",
        12.0,
        72.0,
        1.0,
    ));
    g.add(&spin_row(
        settings,
        "lines",
        "Lines",
        "How many lines of captions to show",
        1.0,
        6.0,
        1.0,
    ));
    g.add(&switch_row(
        settings,
        "dim-drafts",
        "Dim text until it's corrected",
        "Draft text is shown dimmed, then replaced by the corrected sentence",
    ));
    page.add(&g);

    let g = adw::PreferencesGroup::builder().title("Colours").build();
    let presets = adw::ComboRow::builder()
        .title("Style preset")
        .model(&gtk::StringList::new(&[
            "Choose…",
            "White on black",
            "Yellow on black",
            "High contrast",
            "Light",
        ]))
        .build();
    let s = settings.clone();
    presets.connect_selected_notify(move |r| {
        let (fg, bg, op) = match r.selected() {
            1 => ("#ffffff", "#000000", 75),
            2 => ("#ffd60a", "#000000", 85),
            3 => ("#ffffff", "#000000", 100),
            4 => ("#1c1c1c", "#f6f5f4", 90),
            _ => return,
        };
        let _ = s.set_string("text-color", fg);
        let _ = s.set_string("bg-color", bg);
        let _ = s.set_int("bg-opacity", op);
    });
    g.add(&presets);
    g.add(&color_row(settings, "text-color", "Text colour"));
    g.add(&color_row(settings, "bg-color", "Background colour"));
    g.add(&spin_row(
        settings,
        "bg-opacity",
        "Background opacity",
        "Percent",
        0.0,
        100.0,
        5.0,
    ));
    page.add(&g);
    page
}

fn accuracy_page(settings: &gio::Settings) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::new();
    let g = adw::PreferencesGroup::builder().title("Correction").build();
    g.add(&switch_row(
        settings,
        "refine-enabled",
        "Correct each sentence",
        "A second, more accurate model rewrites each sentence after it ends",
    ));
    page.add(&g);
    let g = adw::PreferencesGroup::builder().title("Filters").build();
    g.add(&switch_row(
        settings,
        "profanity-filter",
        "Mask swear words",
        "Shows f*** instead",
    ));
    page.add(&g);
    page.add(&vocab::vocabulary_group(settings));
    page.add(&vocab::replacements_group(settings));
    page
}

fn build(app: &adw::Application) {
    let settings = gio::Settings::new(APP_ID);
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Captionist")
        .default_width(780)
        .default_height(860)
        .build();
    let toast = adw::ToastOverlay::new();

    let client = match Client::connect() {
        Ok(c) => c,
        Err(e) => {
            let page = adw::StatusPage::builder()
                .icon_name("dialog-error-symbolic")
                .title("Caption service not available")
                .description(format!("captionistd could not be started: {}", e.message()))
                .build();
            let view = adw::ToolbarView::new();
            view.add_top_bar(&adw::HeaderBar::new());
            view.set_content(Some(&page));
            window.set_content(Some(&view));
            window.present();
            return;
        }
    };

    let status = adw::ActionRow::builder().title("Status").build();
    let set_status = {
        let status = status.clone();
        move |json: &str| {
            let (text, class) = status_text(json);
            status.set_subtitle(&text);
            for c in ["success", "warning", "error", "dim-label"] {
                status.remove_css_class(c);
            }
            status.add_css_class(class);
        }
    };

    let models = models::ModelsPage::new(client.clone(), settings.clone(), toast.clone());
    let history = history::HistoryPage::new(client.clone(), settings.clone(), toast.clone());

    let stack = adw::ViewStack::new();
    let general = general_page(&settings, &status, &window);
    general.add(&privacy::group(&settings, &client, &toast));
    stack.add_titled_with_icon(
        &general,
        Some("general"),
        "General",
        "emblem-system-symbolic",
    );
    stack.add_titled_with_icon(
        &audio_page(&settings, &client),
        Some("audio"),
        "Audio",
        "audio-speakers-symbolic",
    );
    stack.add_titled_with_icon(
        &models.page,
        Some("models"),
        "Models",
        "applications-science-symbolic",
    );
    stack.add_titled_with_icon(
        &appearance_page(&settings),
        Some("appearance"),
        "Appearance",
        "applications-graphics-symbolic",
    );
    stack.add_titled_with_icon(
        &accuracy_page(&settings),
        Some("accuracy"),
        "Accuracy",
        "emblem-ok-symbolic",
    );
    stack.add_titled_with_icon(
        &history.page,
        Some("history"),
        "History",
        "document-open-recent-symbolic",
    );

    let header = adw::HeaderBar::new();
    let switcher = adw::ViewSwitcher::builder()
        .stack(&stack)
        .policy(adw::ViewSwitcherPolicy::Wide)
        .build();
    header.set_title_widget(Some(&switcher));
    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    toast.set_child(Some(&stack));
    view.set_content(Some(&toast));
    window.set_content(Some(&view));

    {
        let models = models.clone();
        let history = history.clone();
        let set_status = set_status.clone();
        let toast = toast.clone();
        client.on_signal(move |name, params| match name {
            "Final" | "HistoryChanged" => history.on_signal(name),
            "StatusChanged" => {
                if let Some((s,)) = params.get::<(String,)>() {
                    set_status(&s);
                }
            }
            "Error" => {
                if let Some((s,)) = params.get::<(String,)>() {
                    toast.add_toast(adw::Toast::new(&s));
                }
            }
            _ => models.on_signal(name, params),
        });
    }
    {
        let c = client.clone();
        glib::spawn_future_local(async move {
            if let Ok(s) = c.call_string("Status", None).await {
                set_status(&s);
            }
        });
    }
    models.refresh();
    history.refresh();
    privacy::ask_once(&settings, &client, &window);
    for page in ["models", "history", "audio", "accuracy", "appearance"] {
        if std::env::args().any(|a| a == format!("--{page}")) {
            stack.set_visible_child_name(page);
        }
    }
    window.present();
}

fn main() -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    app.connect_command_line(|app, _| {
        if let Some(w) = app.active_window() {
            w.present();
        } else {
            build(app);
        }
        glib::ExitCode::SUCCESS
    });
    app.run()
}
