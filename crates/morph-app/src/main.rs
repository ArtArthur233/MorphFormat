//! Morph-Format desktop application.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod queue;

use morph_core::video::h264_available;
use morph_core::{InputKind, Metadata};
use queue::{
    choices, format_label, ignores_quality, kind_label, run_summary, Queue, Settings, Status,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

const ROW_H: f32 = 32.0;
const ACCENT: egui::Color32 = egui::Color32::from_rgb(0x1F, 0x6F, 0xEB);

fn main() -> eframe::Result {
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
        .expect("embedded icon.png is valid");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_icon(icon)
            .with_inner_size([980.0, 620.0])
            .with_min_inner_size([760.0, 440.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Morph-Format",
        options,
        Box::new(|cc| {
            apply_style(&cc.egui_ctx);
            Ok(Box::<App>::default())
        }),
    )
}

/// Message from the conversion thread to the UI.
enum Event {
    Progress(u64, f32),
    Finished(u64, Status),
    AllDone,
}

/// A batch running on a background thread.
struct Run {
    ids: Vec<u64>,
    rx: Receiver<Event>,
    cancel: Arc<AtomicBool>,
}

#[derive(Default)]
struct App {
    queue: Queue,
    settings: Settings,
    run: Option<Run>,
    notice: String,
}

impl App {
    fn add(&mut self, paths: Vec<PathBuf>) {
        let r = self.queue.add_paths(paths);
        self.notice = match (r.added, r.skipped) {
            (0, 0) => "No files added".into(),
            (a, 0) => format!("{a} added"),
            (a, s) => format!("{a} added · {s} skipped (unsupported format)"),
        };
    }

    fn add_files_dialog(&mut self) {
        let picked = rfd::FileDialog::new().pick_files().unwrap_or_default();
        self.add(picked);
    }

    fn add_folder_dialog(&mut self) {
        let picked = rfd::FileDialog::new().pick_folder();
        self.add(picked.into_iter().collect());
    }

    fn start(&mut self, ctx: &egui::Context) {
        let ids = self.queue.runnable();
        if ids.is_empty() {
            return;
        }
        let jobs: Vec<(u64, PathBuf, InputKind)> = self
            .queue
            .items
            .iter()
            .filter(|i| ids.contains(&i.id))
            .map(|i| (i.id, i.path.clone(), i.kind))
            .collect();
        for &id in &ids {
            self.queue.set_status(id, Status::Pending);
        }
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let (settings, ctx, flag) = (self.settings.clone(), ctx.clone(), cancel.clone());
        std::thread::spawn(move || {
            let options = settings.options();
            for (id, input, kind) in jobs {
                if flag.load(Ordering::Relaxed) {
                    break;
                }
                let format = settings.target(kind);
                // Computed right before converting so two outputs never collide.
                let output = morph_core::output_path(&input, format, &settings.out_dir_for(&input));
                let job = morph_core::Job {
                    input,
                    output: output.clone(),
                    format,
                    options: options.clone(),
                };
                let (ptx, pctx) = (tx.clone(), ctx.clone());
                let progress = move |p: f32| {
                    let _ = ptx.send(Event::Progress(id, p));
                    pctx.request_repaint();
                };
                progress(0.0);
                let status = match morph_core::convert(&job, &progress, &flag) {
                    Ok(()) => Status::Done(output),
                    Err(morph_core::Error::Cancelled) => Status::Pending,
                    Err(e) => Status::Failed(e.to_string()),
                };
                let _ = tx.send(Event::Finished(id, status));
                ctx.request_repaint();
            }
            let _ = tx.send(Event::AllDone);
            ctx.request_repaint();
        });
        self.run = Some(Run { ids, rx, cancel });
    }

    fn poll(&mut self) {
        let Some(run) = &self.run else { return };
        let mut done = false;
        while let Ok(ev) = run.rx.try_recv() {
            match ev {
                Event::Progress(id, p) => self.queue.set_status(id, Status::Running(p)),
                Event::Finished(id, s) => self.queue.set_status(id, s),
                Event::AllDone => done = true,
            }
        }
        if !done {
            return;
        }
        if let Some(run) = self.run.take() {
            let (mut ok, mut failed) = (0, 0);
            for i in self.queue.items.iter().filter(|i| run.ids.contains(&i.id)) {
                match i.status {
                    Status::Done(_) => ok += 1,
                    Status::Failed(_) => failed += 1,
                    _ => {}
                }
            }
            self.notice = run_summary(ok, failed, run.cancel.load(Ordering::Relaxed));
        }
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        let s = &mut self.settings;
        ui.heading("Settings");
        ui.add_space(12.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            // Leave room for the value box ("100 %") so sliders never overflow the panel.
            ui.spacing_mut().slider_width = (ui.available_width() - 72.0).max(80.0);
            ui.spacing_mut().item_spacing.y = 4.0; // label to control; groups get +12
            let mut shown = Vec::new();
            for video in [false, true] {
                if !self.queue.has_kind(video) && (video || self.queue.has_kind(true)) {
                    continue; // show only lists relevant to the queue (images when empty)
                }
                let (label, sel) = if video {
                    ("Video format", &mut s.video_format)
                } else {
                    ("Image format", &mut s.image_format)
                };
                section(ui, label, !shown.is_empty());
                let opts = choices(video);
                if !opts.contains(sel) {
                    if let Some(f) = opts.first() {
                        *sel = *f; // e.g. MP4 default hidden when no H.264 encoder
                    }
                }
                egui::ComboBox::from_id_salt(label)
                    .selected_text(format_label(*sel))
                    .show_ui(ui, |ui| {
                        for f in opts {
                            ui.selectable_value(sel, f, format_label(f));
                        }
                    });
                if video && cfg!(feature = "video") && !h264_available() {
                    ui.label(
                        egui::RichText::new("MP4 unavailable: no H.264 encoder on this system")
                            .small()
                            .weak(),
                    );
                }
                shown.push(*sel);
            }

            section(ui, "Quality", true);
            let unused: Vec<&str> = shown
                .iter()
                .filter(|f| ignores_quality(**f))
                .map(|f| format_label(*f))
                .collect();
            let unused_all = !shown.is_empty() && unused.len() == shown.len();
            ui.add_enabled(!unused_all, egui::Slider::new(&mut s.quality, 1..=100))
                .on_disabled_hover_text(format!("Not used for {}", unused.join(" / ")));

            section(ui, "Resolution", true);
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("resolution")
                    .selected_text(resolution_label(s.resolution))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut s.resolution, None, resolution_label(None));
                        for r in RESOLUTIONS {
                            ui.selectable_value(
                                &mut s.resolution,
                                Some(r),
                                resolution_label(Some(r)),
                            );
                        }
                    });
                help(
                    ui,
                    "Keep original: the size does not change.\n\
                     Otherwise the size is set by the shorter side, like video quality \
                     on YouTube.\n\
                     720p: a 1920 × 1080 video becomes 1280 × 720; \
                     a vertical phone video becomes 720 × 1280.\n\
                     Files already smaller are left as they are.",
                );
            });

            ui.add_space(12.0);
            ui.horizontal(|ui| {
                section(ui, "Metadata", false);
                help(
                    ui,
                    "Hidden information stored inside photos (EXIF): date taken, \
                     camera and settings, and often the GPS location where it was taken.\n\
                     Strip: removes it, safer before sharing.\n\
                     Keep: copies it to the converted file.",
                );
            });
            ui.horizontal(|ui| {
                ui.radio_value(&mut s.metadata, Metadata::Strip, "Strip")
                    .on_hover_text("Removes EXIF, GPS location, camera info");
                ui.radio_value(&mut s.metadata, Metadata::Keep, "Keep");
            });

            section(ui, "Output folder", true);
            let shown = s
                .out_dir
                .as_ref()
                .map_or("Next to the source file".to_string(), |p| {
                    p.display().to_string()
                });
            ui.add(egui::Label::new(shown).truncate());
            ui.horizontal(|ui| {
                if ui.button("Choose…").clicked() {
                    s.out_dir = rfd::FileDialog::new().pick_folder().or(s.out_dir.take());
                }
                if s.out_dir.is_some() && ui.button("Reset").clicked() {
                    s.out_dir = None;
                }
            });
        });
    }

    fn drop_zone(&mut self, ui: &mut egui::Ui) {
        let border = ui.visuals().widgets.noninteractive.bg_stroke.color;
        let mut formats = String::from("Images: PNG, JPEG, WebP, GIF, BMP");
        if cfg!(feature = "heic") {
            formats.push_str(", HEIC");
        }
        if cfg!(feature = "video") {
            formats.push_str(" · Videos: MP4, MOV, MKV, WebM, AVI");
        }
        ui.add_space((ui.available_height() / 2.0 - 110.0).max(0.0));
        ui.vertical_centered(|ui| {
            egui::Frame::new()
                .stroke(egui::Stroke::new(1.5, border))
                .corner_radius(8)
                .inner_margin(32)
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new("Drop images, videos or folders").size(18.0));
                        ui.label(egui::RichText::new(formats).weak());
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui
                                .button("Add files…")
                                .on_hover_text("Add files (Ctrl+O)")
                                .clicked()
                            {
                                self.add_files_dialog();
                            }
                            if ui.button("Add folder…").clicked() {
                                self.add_folder_dialog();
                            }
                        });
                    });
                });
        });
    }

    fn queue_ui(&mut self, ui: &mut egui::Ui) {
        if self.queue.items.is_empty() {
            self.drop_zone(ui);
            return;
        }
        let running = self.run.is_some();
        let link = ui.visuals().hyperlink_color;
        let rtl = egui::Layout::right_to_left(egui::Align::Center);
        let ltr = egui::Layout::left_to_right(egui::Align::Center);
        ui.horizontal(|ui| {
            ui.with_layout(rtl, |ui| {
                let clear = egui::Button::new(egui::RichText::new("Clear finished").color(link))
                    .frame(false);
                if ui
                    .add_enabled(!running && self.queue.has_finished(), clear)
                    .clicked()
                {
                    self.queue.clear_finished();
                }
                ui.with_layout(ltr, |ui| {
                    let n = self.queue.items.len();
                    let files = if n == 1 { "file" } else { "files" };
                    ui.label(egui::RichText::new(format!("{n} {files}")).strong());
                    ui.add(egui::Label::new(egui::RichText::new(&self.notice).weak()).truncate());
                });
            });
        });
        ui.add_space(4.0);

        let settings = &self.settings;
        let mut remove = None;
        let dark = ui.visuals().dark_mode;
        let (ok_color, err_color) = (pal(dark).success, ui.visuals().error_fg_color);
        let stripe = ui.visuals().faint_bg_color;
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (n, item) in self.queue.items.iter().enumerate() {
                let fill = if n % 2 == 1 {
                    stripe
                } else {
                    egui::Color32::TRANSPARENT
                };
                egui::Frame::new()
                    .fill(fill)
                    .corner_radius(4)
                    .inner_margin(egui::Margin::symmetric(8, 0))
                    .show(ui, |ui| {
                        let size = egui::vec2(ui.available_width(), ROW_H);
                        ui.allocate_ui_with_layout(size, rtl, |ui| {
                            let rm = egui::Button::new("✕").min_size(egui::vec2(24.0, 24.0));
                            if ui
                                .add_enabled(!running, rm)
                                .on_hover_text("Remove")
                                .on_disabled_hover_text("Remove")
                                .clicked()
                            {
                                remove = Some(item.id);
                            }
                            ui.allocate_ui_with_layout(egui::vec2(160.0, ROW_H), rtl, |ui| {
                                status_ui(ui, &item.status, ok_color, err_color)
                            });
                            ui.allocate_ui_with_layout(egui::vec2(80.0, ROW_H), ltr, |ui| {
                                let t = format_label(settings.target(item.kind));
                                ui.add(egui::Label::new(format!("→ {t}")).truncate());
                            });
                            ui.allocate_ui_with_layout(egui::vec2(90.0, ROW_H), ltr, |ui| {
                                ui.add(egui::Label::new(kind_label(item.kind)).truncate());
                            });
                            ui.with_layout(ltr, |ui| {
                                let name =
                                    item.path.file_name().unwrap_or_default().to_string_lossy();
                                ui.add(egui::Label::new(name).truncate());
                            });
                        });
                    });
            }
        });
        if let Some(id) = remove {
            self.queue.remove(id);
        }
    }

    fn bottom_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if let Some(run) = &self.run {
            let progress: f32 = self
                .queue
                .items
                .iter()
                .filter(|i| run.ids.contains(&i.id))
                .map(|i| match i.status {
                    Status::Done(_) | Status::Failed(_) => 1.0,
                    Status::Running(p) => p,
                    Status::Pending => 0.0,
                })
                .sum();
            let total = run.ids.len().max(1);
            let current = (progress.floor() as usize + 1).min(total);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Cancel").on_hover_text("Cancel (Esc)").clicked() {
                    run.cancel.store(true, Ordering::Relaxed);
                }
                ui.add(
                    egui::ProgressBar::new(progress / total as f32)
                        .desired_width(ui.available_width())
                        .text(format!("Converting {current} of {total}…")),
                );
            });
            return;
        }
        ui.horizontal(|ui| {
            if !self.queue.items.is_empty() {
                if ui
                    .button("Add files…")
                    .on_hover_text("Add files (Ctrl+O)")
                    .clicked()
                {
                    self.add_files_dialog();
                }
                if ui.button("Add folder…").clicked() {
                    self.add_folder_dialog();
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let can = !self.queue.runnable().is_empty();
                let go = egui::Button::new(
                    egui::RichText::new("Convert")
                        .strong()
                        .color(egui::Color32::WHITE),
                )
                .fill(ACCENT)
                .min_size(egui::vec2(140.0, 36.0));
                let tip = "Convert (Ctrl+Enter)";
                if ui
                    .add_enabled(can, go)
                    .on_hover_text(tip)
                    .on_disabled_hover_text(tip)
                    .clicked()
                {
                    self.start(ctx);
                }
                if let Some(dir) = self.queue.last_output_dir() {
                    if ui.button("Open output folder").clicked() {
                        open_folder(&dir);
                    }
                }
            });
        });
    }
}

/// Resolution presets: shorter side in pixels.
const RESOLUTIONS: [u32; 6] = [2160, 1440, 1080, 720, 480, 360];

fn resolution_label(r: Option<u32>) -> String {
    match r {
        None => "Keep original".into(),
        Some(2160) => "4K (2160p)".into(),
        Some(r) => format!("{r}p"),
    }
}

/// Small round "?" that explains a setting on hover, and keeps it open on click.
fn help(ui: &mut egui::Ui, text: &str) {
    let r = ui.add(egui::Button::new("?").small().corner_radius(8));
    let open = egui::Popup::from_toggle_button_response(&r)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .width(260.0)
        .show(|ui| ui.label(text))
        .is_some();
    if !open {
        r.on_hover_text(text);
    }
}

/// Small bold weak label opening a settings group (`gap` adds the space between groups).
fn section(ui: &mut egui::Ui, text: &str, gap: bool) {
    if gap {
        ui.add_space(12.0);
    }
    ui.label(egui::RichText::new(text).small().strong().weak());
}

/// Status cell: symbol + word, so colour is never the only cue.
fn status_ui(ui: &mut egui::Ui, status: &Status, ok: egui::Color32, err: egui::Color32) {
    match status {
        Status::Pending => {
            ui.label(egui::RichText::new("• Waiting").weak());
        }
        Status::Running(p) => {
            ui.add(
                egui::ProgressBar::new(*p)
                    .desired_width(ui.available_width())
                    .show_percentage(),
            );
        }
        Status::Done(path) => {
            if ui.button("Show").clicked() {
                if let Some(dir) = path.parent() {
                    open_folder(dir);
                }
            }
            let out = path.file_name().unwrap_or_default().to_string_lossy();
            let text = egui::RichText::new(format!("✔ Done → {out}")).color(ok);
            ui.add(egui::Label::new(text).truncate())
                .on_hover_text(path.display().to_string());
        }
        Status::Failed(msg) => {
            let text = egui::RichText::new(format!("⚠ Failed: {msg}")).color(err);
            ui.add(egui::Label::new(text).truncate()).on_hover_text(msg);
        }
    }
}

struct Palette {
    central: egui::Color32,
    side: egui::Color32,
    stripe: egui::Color32,
    border: egui::Color32,
    text: egui::Color32,
    weak: egui::Color32,
    link: egui::Color32,
    success: egui::Color32,
    error: egui::Color32,
}

fn pal(dark: bool) -> Palette {
    let c = |h: u32| egui::Color32::from_rgb((h >> 16) as u8, (h >> 8) as u8, h as u8);
    if dark {
        Palette {
            central: c(0x17181A),
            side: c(0x1F2023),
            stripe: c(0x222428),
            border: c(0x34373C),
            text: c(0xE6E8EB),
            weak: c(0xA0A6AE),
            link: c(0x6CA6FF),
            success: c(0x3FB950),
            error: c(0xF85149),
        }
    } else {
        Palette {
            central: c(0xFFFFFF),
            side: c(0xF6F7F9),
            stripe: c(0xF2F4F7),
            border: c(0xD8DCE1),
            text: c(0x1F2328),
            weak: c(0x5C6370),
            link: c(0x0B5CD5),
            success: c(0x1A7F37),
            error: c(0xCF222E),
        }
    }
}

fn visuals(dark: bool) -> egui::Visuals {
    let p = pal(dark);
    let mut v = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.panel_fill = p.central;
    v.window_fill = p.central;
    v.faint_bg_color = p.stripe;
    v.weak_text_color = Some(p.weak);
    v.hyperlink_color = p.link;
    v.error_fg_color = p.error;
    v.selection.bg_fill = ACCENT;
    // Deviation from the report: egui draws selected text with this colour on the accent fill,
    // so white (not accent) keeps it readable.
    v.selection.stroke = egui::Stroke::new(1.5, egui::Color32::WHITE);
    v.window_corner_radius = 8.into();
    v.widgets.noninteractive.bg_stroke.color = p.border;
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.fg_stroke.color = p.text;
        w.corner_radius = 6.into();
    }
    v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, p.border);
    v
}

/// Applies the type scale, spacing and per-theme colours once at startup.
fn apply_style(ctx: &egui::Context) {
    use egui::{FontFamily::Proportional, FontId, TextStyle};
    ctx.set_visuals_of(egui::Theme::Light, visuals(false));
    ctx.set_visuals_of(egui::Theme::Dark, visuals(true));
    ctx.all_styles_mut(|style| {
        for (ts, size) in [
            (TextStyle::Small, 12.0),
            (TextStyle::Body, 14.0),
            (TextStyle::Button, 14.0),
            (TextStyle::Heading, 18.0),
        ] {
            style
                .text_styles
                .insert(ts, FontId::new(size, Proportional));
        }
        style
            .text_styles
            .insert(TextStyle::Monospace, FontId::monospace(13.0));
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.interact_size.y = 28.0;
        style.spacing.slider_width = 200.0;
        // Show help tooltips quickly, even if the cursor moves slightly.
        style.interaction.tooltip_delay = 0.2;
        style.interaction.show_tooltips_only_when_still = false;
    });
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll();

        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if !dropped.is_empty() {
            self.add(dropped);
        }
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        let running = self.run.is_some();

        let (open, go, esc) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::O),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            )
        });
        if let Some(run) = &self.run {
            if esc {
                run.cancel.store(true, Ordering::Relaxed);
            }
        } else {
            if open {
                self.add_files_dialog();
            }
            if go {
                self.start(&ctx);
            }
        }

        let side = pal(ctx.theme() == egui::Theme::Dark).side;
        // Bottom first so the bar spans the full width.
        egui::Panel::bottom("actions")
            .frame(
                egui::Frame::new()
                    .fill(side)
                    .inner_margin(egui::Margin::symmetric(16, 10)),
            )
            .show(ui, |ui| self.bottom_bar(ui, &ctx));

        egui::Panel::right("settings")
            .exact_size(280.0)
            .frame(egui::Frame::new().fill(side).inner_margin(16))
            .show(ui, |ui| {
                ui.add_enabled_ui(!running, |ui| self.settings_ui(ui));
            });

        egui::CentralPanel::default_margins().show(ui, |ui| self.queue_ui(ui));

        if hovering {
            let rect = ctx.content_rect();
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::unique("drop"),
            ));
            painter.rect_filled(rect, 0.0, egui::Color32::from_black_alpha(160));
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                if running {
                    "Will be added; convert after this batch"
                } else {
                    "Drop to add to the queue"
                },
                egui::FontId::proportional(28.0),
                egui::Color32::WHITE,
            );
        }
    }
}

/// Opens `dir` in the system file manager.
fn open_folder(dir: &std::path::Path) {
    let opener = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(opener).arg(dir).spawn();
}
