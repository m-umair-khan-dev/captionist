//! Settings → Models: hardware, recommendation, sortable colour-coded model list.

use crate::client::Client;
use crate::widgets::choice_row;
use adw::prelude::*;
use captionist_core::accel::{Advice, Kind};
use captionist_core::catalog::Role;
use captionist_core::listing::{Fit, ModelRow, ModelsReport, SpeedSource};
use gtk::{gio, glib};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub struct ModelsPage {
    pub page: adw::PreferencesPage,
    client: Client,
    settings: gio::Settings,
    toast: adw::ToastOverlay,
    pc_group: adw::PreferencesGroup,
    rec_group: adw::PreferencesGroup,
    live_group: adw::PreferencesGroup,
    refine_group: adw::PreferencesGroup,
    accel_group: adw::PreferencesGroup,
    /// Rows added by the last refresh (removed on the next one).
    dynamic: RefCell<Vec<(adw::PreferencesGroup, gtk::Widget)>>,
    /// Download progress labels by model id.
    progress: RefCell<HashMap<String, gtk::Label>>,
    last: RefCell<Option<ModelsReport>>,
}

const SORTS: &[(&str, &str)] = &[
    ("recommended", "Recommended"),
    ("accuracy", "Most accurate"),
    ("speed", "Fastest"),
    ("download", "Smallest download"),
    ("memory", "Least memory"),
];
const MODES: &[(&str, &str)] = &[("auto", "Automatic (recommended)"), ("manual", "Manual")];

fn mb(v: u64) -> String {
    if v >= 1024 {
        format!("{:.1} GB", v as f64 / 1024.0)
    } else {
        format!("{v} MB")
    }
}

fn speed_text(r: &ModelRow) -> String {
    match (r.rtf, r.speed_source) {
        (Some(v), SpeedSource::Measured) => format!("{:.1}× real time", 1.0 / v),
        (Some(v), _) => format!("~{:.1}× real time (estimated)", 1.0 / v),
        (None, _) => "speed not tested".into(),
    }
}

impl ModelsPage {
    pub fn new(client: Client, settings: gio::Settings, toast: adw::ToastOverlay) -> Rc<Self> {
        let page = adw::PreferencesPage::new();

        let pc_group = adw::PreferencesGroup::builder().title("This PC").build();
        let recheck = gtk::Button::builder()
            .label("Re-check")
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        recheck.set_tooltip_text(Some(
            "Detect hardware again and re-time the installed models",
        ));
        pc_group.set_header_suffix(Some(&recheck));

        let rec_group = adw::PreferencesGroup::builder()
            .title("Recommended for this PC")
            .build();

        let opts = adw::PreferencesGroup::builder()
            .title("Model choice")
            .build();
        opts.add(&choice_row(
            &settings,
            "model-mode",
            "Mode",
            "Automatic picks the best models your PC can run smoothly",
            MODES,
        ));
        opts.add(&choice_row(
            &settings,
            "model-sort",
            "Sort list by",
            "",
            SORTS,
        ));

        let live_group = adw::PreferencesGroup::builder()
            .title("Live captions")
            .description("Shows text while someone is speaking")
            .build();
        let refine_group = adw::PreferencesGroup::builder()
            .title("Correction")
            .description("Re-reads each sentence after it ends and fixes mistakes")
            .build();

        let accel_group = adw::PreferencesGroup::builder()
            .title("Acceleration (optional)")
            .description("Captionist checked this PC for graphics chips and NPUs that can run the correction model. Set up downloads the matching runtime and measures whether it's actually faster here.")
            .build();

        for g in [
            &pc_group,
            &rec_group,
            &opts,
            &live_group,
            &refine_group,
            &accel_group,
        ] {
            page.add(g);
        }

        let me = Rc::new(ModelsPage {
            page,
            client,
            settings: settings.clone(),
            toast,
            pc_group,
            rec_group,
            live_group,
            refine_group,
            accel_group,
            dynamic: RefCell::new(vec![]),
            progress: RefCell::new(HashMap::new()),
            last: RefCell::new(None),
        });

        let m = me.clone();
        recheck.connect_clicked(move |_| m.call("Recheck", None));
        let m = me.clone();
        settings.connect_changed(None, move |_, key| {
            if matches!(
                key,
                "model-sort" | "model-mode" | "live-model" | "refine-model" | "accelerator"
            ) {
                m.refresh();
            }
        });
        me
    }

    fn error(&self, msg: &str) {
        self.toast.add_toast(adw::Toast::new(msg));
    }

    fn call(self: &Rc<Self>, method: &'static str, args: Option<glib::Variant>) {
        let me = self.clone();
        self.client.spawn_call(method, args, move |e| me.error(&e));
    }

    /// Signal handler: refresh on ModelsChanged, update progress labels.
    pub fn on_signal(self: &Rc<Self>, name: &str, params: &glib::Variant) {
        match name {
            "ModelsChanged" => self.refresh(),
            "DownloadProgress" => {
                if let Some((id, p)) = params.get::<(String, f64)>()
                    && let Some(l) = self.progress.borrow().get(&id)
                {
                    l.set_text(&format!("Downloading {:.0}%", p * 100.0));
                }
            }
            _ => {}
        }
    }

    pub fn refresh(self: &Rc<Self>) {
        let me = self.clone();
        let sort = self.settings.string("model-sort").to_string();
        glib::spawn_future_local(async move {
            match me
                .client
                .call_string("ListModels", Some((sort,).to_variant()))
                .await
            {
                Ok(json) => match serde_json::from_str::<ModelsReport>(&json) {
                    Ok(r) => me.rebuild(r),
                    Err(e) => me.error(&format!("Unexpected reply from the caption service: {e}")),
                },
                Err(e) => me.error(&format!("Caption service not reachable: {}", e.message())),
            }
        });
    }

    fn add(&self, group: &adw::PreferencesGroup, w: &impl IsA<gtk::Widget>) {
        group.add(w);
        self.dynamic
            .borrow_mut()
            .push((group.clone(), w.clone().upcast()));
    }

    fn rebuild(self: &Rc<Self>, r: ModelsReport) {
        for (g, w) in self.dynamic.borrow_mut().drain(..) {
            g.remove(&w);
        }
        self.progress.borrow_mut().clear();

        // ── This PC
        let hw = &r.hardware;
        let gpu = hw
            .gpus
            .first()
            .map(|g| {
                format!(
                    "{}{}",
                    g.name,
                    if g.compute_runtime {
                        ""
                    } else {
                        " (no compute driver)"
                    }
                )
            })
            .unwrap_or_else(|| "no GPU".into());
        let hw_row = adw::ActionRow::builder()
            .title(&hw.cpu.model)
            .subtitle(format!(
                "{} cores · {:.1} GB RAM · {}",
                hw.cpu.physical_cores,
                hw.memory.total_mb as f64 / 1024.0,
                gpu
            ))
            .use_markup(false)
            .build();
        hw_row.add_prefix(&gtk::Image::from_icon_name("computer-symbolic"));
        self.add(&self.pc_group, &hw_row);

        let speed = match r.factor {
            Some(f) if f > 1.05 => {
                format!("Runs speech models {f:.1}× slower than the reference PC (Intel N150)")
            }
            Some(f) if f < 0.95 => format!(
                "Runs speech models {:.1}× faster than the reference PC (Intel N150)",
                1.0 / f
            ),
            Some(_) => "Runs speech models about as fast as the reference PC (Intel N150)".into(),
            None => "Not checked yet: speeds unknown".into(),
        };
        let speed_row = adw::ActionRow::builder()
            .title("Speed")
            .subtitle(speed)
            .build();
        speed_row.add_prefix(&gtk::Image::from_icon_name("speedometer-symbolic"));
        self.add(&self.pc_group, &speed_row);

        if let Some(b) = &r.busy {
            let busy = adw::ActionRow::builder().title(b).build();
            let sp = gtk::Spinner::builder()
                .spinning(true)
                .valign(gtk::Align::Center)
                .build();
            busy.add_prefix(&sp);
            self.add(&self.pc_group, &busy);
        }

        // ── Recommendation
        let rec = &r.recommendation;
        let names: Vec<String> = [&rec.live, &rec.refine]
            .into_iter()
            .flatten()
            .map(|c| c.model_name.clone())
            .collect();
        let rec_row = adw::ActionRow::builder()
            .title(if names.is_empty() {
                "Checking this PC…".to_string()
            } else {
                names.join(" + ")
            })
            .subtitle(match (&rec.live, &rec.refine) {
                (Some(_), Some(_)) => format!(
                    "{:?} tier · live captions, then each sentence corrected",
                    rec.tier
                ),
                (Some(_), None) => format!("{:?} tier · live captions only", rec.tier),
                _ => String::new(),
            })
            .build();
        rec_row.add_prefix(
            &gtk::Image::builder()
                .icon_name("starred-symbolic")
                .css_classes(["warning"])
                .build(),
        );
        let missing: Vec<&ModelRow> = r
            .rows
            .iter()
            .filter(|row| row.recommended && !row.installed)
            .collect();
        let auto = self.settings.string("model-mode") == "auto";
        if !missing.is_empty() {
            let total: u32 = missing.iter().map(|m| m.download_mb).sum();
            let b = gtk::Button::builder()
                .label(format!("Download ({})", mb(total as u64)))
                .valign(gtk::Align::Center)
                .css_classes(["suggested-action"])
                .build();
            let me = self.clone();
            b.connect_clicked(move |b| {
                b.set_sensitive(false);
                let _ = me.settings.set_string("model-mode", "auto");
                me.call("DownloadRecommended", None);
            });
            rec_row.add_suffix(&b);
        } else if !auto {
            let b = gtk::Button::builder()
                .label("Use recommended")
                .valign(gtk::Align::Center)
                .build();
            let s = self.settings.clone();
            b.connect_clicked(move |_| {
                let _ = s.set_string("model-mode", "auto");
            });
            rec_row.add_suffix(&b);
        } else if !names.is_empty() {
            rec_row.add_suffix(
                &gtk::Label::builder()
                    .label("In use")
                    .css_classes(["success", "heading"])
                    .build(),
            );
        }
        self.add(&self.rec_group, &rec_row);
        for n in &rec.notes {
            let note = adw::ActionRow::builder()
                .subtitle(n)
                .use_markup(false)
                .build();
            note.add_prefix(&gtk::Image::from_icon_name("dialog-information-symbolic"));
            self.add(&self.rec_group, &note);
        }

        // ── Model lists
        let downloading: HashMap<String, f64> = r.downloading.iter().cloned().collect();
        for row in &r.rows {
            let group = match row.role {
                Role::Live => &self.live_group,
                Role::Refine => &self.refine_group,
            };
            let active = match row.role {
                Role::Live => r.active_live.as_deref() == Some(&row.id),
                Role::Refine => r.active_refine.as_deref() == Some(&row.id),
            };
            let w = self.model_row(row, active, downloading.get(&row.id).copied(), &r);
            self.add(group, &w);
        }
        self.accel_rows(&r);
        *self.last.borrow_mut() = Some(r);
    }

    fn accel_rows(self: &Rc<Self>, r: &ModelsReport) {
        let g = &self.accel_group;
        let label = |text: &str, class: &str| {
            gtk::Label::builder()
                .label(text)
                .css_classes([class, "caption-heading"])
                .valign(gtk::Align::Center)
                .build()
        };
        if r.accel.is_empty() {
            let row = adw::ActionRow::builder()
                .title("No supported accelerator found")
                .subtitle("Captions run on the processor. Supported: NVIDIA GPUs, Intel graphics and Intel NPUs.")
                .build();
            row.add_prefix(&gtk::Image::from_icon_name("processor-symbolic"));
            self.add(g, &row);
            return;
        }
        let mut restarting = false;
        for o in &r.accel {
            let title = o
                .kind
                .map(|k| k.label().to_string())
                .unwrap_or_else(|| o.device.clone());
            let row = adw::ActionRow::builder()
                .title(&title)
                .subtitle(format!("{}\n{}", o.device, o.message))
                .subtitle_lines(3)
                .subtitle_selectable(o.advice == Advice::NeedsDriver)
                .use_markup(false)
                .build();
            row.add_prefix(&gtk::Image::from_icon_name(
                if o.kind == Some(Kind::IntelNpu) {
                    "processor-symbolic"
                } else {
                    "video-display-symbolic"
                },
            ));
            row.add_suffix(&label(o.advice.label(), o.advice.css_class()));

            let pack_key = o.pack.as_ref().map(|(id, _, _)| format!("pack:{id}"));
            let downloading = pack_key
                .as_ref()
                .and_then(|k| r.downloading.iter().find(|(id, _)| id == k));
            let set = |v: &'static str| {
                let s = self.settings.clone();
                move |_: &gtk::Button| {
                    let _ = s.set_string("accelerator", v);
                }
            };
            if let (Some(kind), true) = (o.kind, o.driver_ok) {
                let strong = matches!(o.advice, Advice::Recommended | Advice::Faster);
                if let Some((_, p)) = downloading {
                    let l = gtk::Label::builder()
                        .label(format!("Downloading {:.0}%", p * 100.0))
                        .css_classes(["dim-label"])
                        .valign(gtk::Align::Center)
                        .build();
                    self.progress
                        .borrow_mut()
                        .insert(pack_key.clone().unwrap_or_default(), l.clone());
                    row.add_suffix(&l);
                } else if o.active {
                    row.add_suffix(&label("In use", "accent"));
                    let b = gtk::Button::builder()
                        .label("Turn off")
                        .valign(gtk::Align::Center)
                        .build();
                    b.connect_clicked(set("cpu"));
                    row.add_suffix(&b);
                } else if o.enabled && o.pack_installed {
                    row.add_suffix(&label("Starting…", "dim-label"));
                    restarting = true;
                } else if !o.pack_installed {
                    let mb = o.pack.as_ref().map(|p| p.2).unwrap_or(0);
                    let b = gtk::Button::builder()
                        .label(format!("Set up ({mb} MB)"))
                        .valign(gtk::Align::Center)
                        .build();
                    b.set_tooltip_text(Some(
                        "Downloads the runtime, switches to it and measures the speed",
                    ));
                    if strong || o.advice == Advice::WorthTesting {
                        b.add_css_class("suggested-action");
                    }
                    let (me, s, id) = (self.clone(), self.settings.clone(), kind.id());
                    b.connect_clicked(move |b| {
                        b.set_sensitive(false);
                        let _ = s.set_string("accelerator", id);
                        me.call("InstallAccelerator", Some((id.to_string(),).to_variant()));
                    });
                    row.add_suffix(&b);
                } else {
                    let b = gtk::Button::builder()
                        .label("Turn on")
                        .valign(gtk::Align::Center)
                        .build();
                    if strong {
                        b.add_css_class("suggested-action");
                    }
                    b.connect_clicked(set(kind.id()));
                    row.add_suffix(&b);
                    if let Some((id, _, _)) = &o.pack {
                        let del = gtk::Button::builder()
                            .icon_name("user-trash-symbolic")
                            .valign(gtk::Align::Center)
                            .css_classes(["flat"])
                            .tooltip_text("Remove the runtime pack")
                            .build();
                        let (me, id) = (self.clone(), id.clone());
                        del.connect_clicked(move |_| {
                            me.call("RemoveAccelerator", Some((id.clone(),).to_variant()))
                        });
                        row.add_suffix(&del);
                    }
                }
            }
            self.add(g, &row);

            for (name, cpu, acc) in &o.results {
                let (text, t, class) = match (cpu, acc) {
                    (_, None) => (
                        "Couldn't run here; stays on the processor".to_string(),
                        "Failed",
                        "error",
                    ),
                    (Some(c), Some(a)) => {
                        let text = format!(
                            "{:.1}× real time here vs {:.1}× on the processor",
                            1.0 / a,
                            1.0 / c
                        );
                        if *a < c * 0.87 {
                            (text, "Faster", "success")
                        } else if *a > c * 1.1 {
                            (text, "Slower", "warning")
                        } else {
                            (text, "Same", "dim-label")
                        }
                    }
                    (None, Some(a)) => (
                        format!("{:.1}× real time", 1.0 / a),
                        "Measured",
                        "dim-label",
                    ),
                };
                let sub = adw::ActionRow::builder().title(name).subtitle(text).build();
                sub.add_prefix(&gtk::Image::from_icon_name("speedometer-symbolic"));
                sub.add_suffix(&label(t, class));
                self.add(g, &sub);
            }
        }
        if restarting {
            // The service restarts to load the accelerator runtime; reconnect shortly.
            let me = self.clone();
            glib::timeout_add_seconds_local_once(5, move || {
                me.call("Ping", None);
                me.refresh();
            });
        }
    }

    fn model_row(
        self: &Rc<Self>,
        r: &ModelRow,
        active: bool,
        downloading: Option<f64>,
        report: &ModelsReport,
    ) -> adw::ActionRow {
        let acc = r
            .accuracy_pct
            .map(|a| format!("{a:.1}% accurate"))
            .unwrap_or_else(|| "accuracy not measured".into());
        let ram = r
            .ram_mb
            .map(|m| format!(" · {} RAM", mb(m)))
            .unwrap_or_default();
        let row = adw::ActionRow::builder()
            .title(&r.name)
            .subtitle(format!(
                "{}\n{acc} · {}{ram} · {} download",
                r.summary,
                speed_text(r),
                mb(r.download_mb as u64)
            ))
            .subtitle_lines(3)
            .use_markup(false)
            .build();

        // ★ recommended (amber star), else an empty slot so names line up.
        let star = gtk::Image::from_icon_name(if r.recommended {
            "starred-symbolic"
        } else {
            "non-starred-symbolic"
        });
        star.add_css_class(if r.recommended {
            "warning"
        } else {
            "dim-label"
        });
        star.set_tooltip_text(Some(if r.recommended {
            "Recommended for this PC"
        } else {
            ""
        }));
        row.add_prefix(&star);

        // Colour-coded fit: green / amber / red.
        let fit = gtk::Label::builder()
            .label(r.fit.label())
            .css_classes([r.fit.css_class(), "caption-heading"])
            .valign(gtk::Align::Center)
            .build();
        if !r.fit_message.is_empty() {
            fit.set_tooltip_text(Some(&r.fit_message));
        }
        row.add_suffix(&fit);

        if let Some(p) = downloading {
            let l = gtk::Label::builder()
                .label(format!("Downloading {:.0}%", p * 100.0))
                .css_classes(["dim-label"])
                .valign(gtk::Align::Center)
                .build();
            self.progress.borrow_mut().insert(r.id.clone(), l.clone());
            row.add_suffix(&l);
        } else if !r.installed {
            let b = gtk::Button::builder()
                .label("Download")
                .valign(gtk::Align::Center)
                .build();
            if r.recommended {
                b.add_css_class("suggested-action");
            }
            let (me, id) = (self.clone(), r.id.clone());
            b.connect_clicked(move |b| {
                b.set_sensitive(false);
                me.call("DownloadModel", Some((id.clone(),).to_variant()));
            });
            row.add_suffix(&b);
        } else if active {
            row.add_suffix(
                &gtk::Label::builder()
                    .label("In use")
                    .css_classes(["accent", "heading"])
                    .valign(gtk::Align::Center)
                    .build(),
            );
        } else {
            let b = gtk::Button::builder()
                .label("Use")
                .valign(gtk::Align::Center)
                .build();
            let (me, model, act_live, act_ref) = (
                self.clone(),
                r.clone(),
                report.active_live.clone(),
                report.active_refine.clone(),
            );
            b.connect_clicked(move |b| {
                me.confirm_use(b, &model, act_live.clone(), act_ref.clone())
            });
            row.add_suffix(&b);

            // The calibration model can't be deleted: it's needed for speed estimates.
            let is_cal = captionist_core::catalog::Catalog::embedded()
                .calibration_model()
                .is_some_and(|c| c.id == r.id);
            if !is_cal {
                let del = gtk::Button::builder()
                    .icon_name("user-trash-symbolic")
                    .valign(gtk::Align::Center)
                    .css_classes(["flat"])
                    .tooltip_text("Delete this model")
                    .build();
                let (me, id) = (self.clone(), r.id.clone());
                del.connect_clicked(move |_| {
                    me.call("DeleteModel", Some((id.clone(),).to_variant()))
                });
                row.add_suffix(&del);
            }
        }
        row
    }

    /// Switch to manual mode with this model; asks first if it may lag or is too slow.
    fn confirm_use(
        self: &Rc<Self>,
        anchor: &gtk::Button,
        m: &ModelRow,
        act_live: Option<String>,
        act_ref: Option<String>,
    ) {
        let apply = {
            let s = self.settings.clone();
            let m = m.clone();
            move || {
                let _ = s.set_string("model-mode", "manual");
                match m.role {
                    Role::Live => {
                        let _ = s.set_string("live-model", &m.id);
                        if s.string("refine-model").is_empty() {
                            let _ = s.set_string("refine-model", act_ref.as_deref().unwrap_or(""));
                        }
                    }
                    Role::Refine => {
                        let _ = s.set_string("refine-model", &m.id);
                        if s.string("live-model").is_empty() {
                            let _ = s.set_string("live-model", act_live.as_deref().unwrap_or(""));
                        }
                    }
                }
            }
        };
        if matches!(m.fit, Fit::Smooth | Fit::Unknown) {
            apply();
            return;
        }
        let dialog = adw::AlertDialog::builder()
            .heading(format!("Use {}?", m.name))
            .body(format!(
                "{}\n\nYou can switch back any time with \"Use recommended\".",
                m.fit_message
            ))
            .build();
        dialog.add_responses(&[("cancel", "Cancel"), ("use", "Use anyway")]);
        dialog.set_response_appearance(
            "use",
            if m.fit == Fit::TooSlow {
                adw::ResponseAppearance::Destructive
            } else {
                adw::ResponseAppearance::Suggested
            },
        );
        dialog.set_default_response(Some("cancel"));
        dialog.connect_response(None, move |_, resp| {
            if resp == "use" {
                apply();
            }
        });
        dialog.present(anchor.root().as_ref());
    }
}
