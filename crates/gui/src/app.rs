//! `App` state + the `eframe::App` implementation.
//!
//! Threading model: the `core_convert` function is CPU-bound (JBIG
//! decode + PDF assembly), so we never call it on the UI thread. The
//! "Convert all" button spawns one `std::thread` per pending file;
//! each worker pushes a [`WorkerMsg`] back through the channel. The
//! UI calls [`App::poll_workers`] at the start of every frame to
//! drain the channel and update statuses.
//!
//! Visual design: card-based layout, color-coded status badges, drag
//! overlay that lights up when files are hovering, generous spacing,
//! and a soft neutral palette that works in both light and dark
//! themes. Inspired by macOS Sonoma / Windows 11 file dialogs.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::thread::{self, JoinHandle};

use anyhow::{Context, Result};
use eframe::egui::{
    self, Color32, CornerRadius, CursorIcon, Frame, Margin, Response, RichText, Sense, Shadow,
    Stroke, StrokeKind, Ui, Vec2,
};

use caj2pdf_core::convert::convert as core_convert;

use crate::font::is_being_dragged;

// ---------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------

mod theme {
    use eframe::egui::Color32;

    /// Page background — almost-white in light mode, deep grey in dark.
    pub const PAGE_BG: Color32 = Color32::from_rgb(248, 250, 252);
    pub const CARD_BG: Color32 = Color32::from_rgb(255, 255, 255);
    pub const CARD_BORDER: Color32 = Color32::from_rgb(226, 232, 240);
    pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(15, 23, 42);
    pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(100, 116, 139);
    pub const ACCENT: Color32 = Color32::from_rgb(59, 130, 246);
    pub const ACCENT_DARK: Color32 = Color32::from_rgb(37, 99, 235);

    pub const STATUS_PENDING_BG: Color32 = Color32::from_rgb(241, 245, 249);
    pub const STATUS_PENDING_FG: Color32 = Color32::from_rgb(71, 85, 105);

    pub const STATUS_RUNNING_BG: Color32 = Color32::from_rgb(219, 234, 254);
    pub const STATUS_RUNNING_FG: Color32 = Color32::from_rgb(29, 78, 216);

    pub const STATUS_DONE_BG: Color32 = Color32::from_rgb(220, 252, 231);
    pub const STATUS_DONE_FG: Color32 = Color32::from_rgb(22, 101, 52);

    pub const STATUS_ERROR_BG: Color32 = Color32::from_rgb(254, 226, 226);
    pub const STATUS_ERROR_FG: Color32 = Color32::from_rgb(153, 27, 27);

    pub const DRAG_BG: Color32 = Color32::from_rgb(239, 246, 255);
    pub const DRAG_BORDER: Color32 = Color32::from_rgb(147, 197, 253);
    pub const DRAG_BORDER_STRONG: Color32 = Color32::from_rgb(59, 130, 246);
}

// ---------------------------------------------------------------------------
// Public data model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub input: PathBuf,
    pub status: Status,
}

#[derive(Debug, Clone)]
pub enum Status {
    Pending,
    Running,
    Done(PathBuf),
    Failed(String),
}

impl Status {
    fn badge(&self) -> (&'static str, Color32, Color32) {
        match self {
            Status::Pending => (
                "待处理",
                theme::STATUS_PENDING_BG,
                theme::STATUS_PENDING_FG,
            ),
            Status::Running => (
                "转换中…",
                theme::STATUS_RUNNING_BG,
                theme::STATUS_RUNNING_FG,
            ),
            Status::Done(_) => (
                "完成",
                theme::STATUS_DONE_BG,
                theme::STATUS_DONE_FG,
            ),
            Status::Failed(_) => (
                "失败",
                theme::STATUS_ERROR_BG,
                theme::STATUS_ERROR_FG,
            ),
        }
    }

    fn icon(&self) -> &'static str {
        match self {
            Status::Pending => "○",
            Status::Running => "↻",
            Status::Done(_) => "✓",
            Status::Failed(_) => "✕",
        }
    }
}

// ---------------------------------------------------------------------------
// Worker pool plumbing
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum WorkerMsg {
    Started(PathBuf),
    Finished {
        input: PathBuf,
        result: Result<PathBuf, String>,
    },
}

#[derive(Debug, Default)]
pub struct App {
    files: Vec<FileEntry>,
    rx: Option<Receiver<WorkerMsg>>,
    workers: Vec<JoinHandle<()>>,
}

impl App {
    /// Add `path` to the list, ignoring duplicates.
    pub fn add_path(&mut self, path: PathBuf) {
        if !self.files.iter().any(|f| f.input == path) {
            self.files.push(FileEntry {
                input: path,
                status: Status::Pending,
            });
        }
    }

    /// Drain the worker channel and update each row's status.
    fn poll_workers(&mut self) {
        let Some(rx) = &self.rx else {
            return;
        };
        while let Ok(msg) = rx.try_recv() {
            match msg {
                WorkerMsg::Started(p) => {
                    if let Some(e) = self.files.iter_mut().find(|e| e.input == p) {
                        e.status = Status::Running;
                    }
                }
                WorkerMsg::Finished { input, result } => {
                    if let Some(e) = self.files.iter_mut().find(|e| e.input == input) {
                        e.status = match result {
                            Ok(out) => Status::Done(out),
                            Err(e) => Status::Failed(e),
                        };
                    }
                }
            }
        }
        // Drop finished handles.
        self.workers.retain(|h| !h.is_finished());
    }

    /// Spawn one thread per Pending / Failed file to run `core_convert`.
    fn start_conversion(&mut self, ctx: egui::Context) {
        let (tx, rx) = channel::<WorkerMsg>();
        self.rx = Some(rx);

        let todo: Vec<FileEntry> = self
            .files
            .iter()
            .filter(|e| matches!(e.status, Status::Pending | Status::Failed(_)))
            .cloned()
            .collect();

        for entry in todo {
            let tx = tx.clone();
            let ctx = ctx.clone();
            self.workers.push(thread::spawn(move || {
                let _ = tx.send(WorkerMsg::Started(entry.input.clone()));
                let result = convert_one(&entry.input).map_err(|e| format!("{:#}", e));
                let _ = tx.send(WorkerMsg::Finished {
                    input: entry.input,
                    result,
                });
                ctx.request_repaint();
            }));
        }
    }
}

/// Convert one file, mapping errors to a printable form.
fn convert_one(input: &Path) -> Result<PathBuf> {
    let mut out = input.to_path_buf();
    out.set_extension("pdf");
    core_convert(input, &out)
        .map_err(anyhow::Error::new)
        .with_context(|| format!("converting {} -> {}", input.display(), out.display()))?;
    Ok(out)
}

// ---------------------------------------------------------------------------
// Drawing helpers
// ---------------------------------------------------------------------------

/// Render a rounded card. Returns the inner Response.
fn card(ui: &mut Ui, bg: Color32, border: Color32, add: impl FnOnce(&mut Ui)) -> Response {
    Frame::group(ui.style())
        .fill(bg)
        .stroke(Stroke::new(1.0, border))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(16))
        .shadow(Shadow {
            offset: [0, 1],
            blur: 3,
            spread: 0,
            color: Color32::from_black_alpha(8),
        })
        .show(ui, add)
        .response
}

/// A pill-shaped status badge: "[icon]  label"
fn status_badge(ui: &mut Ui, status: &Status) {
    let (label, bg, fg) = status.badge();
    Frame::group(ui.style())
        .fill(bg)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(
                    RichText::new(status.icon())
                        .monospace()
                        .color(fg)
                        .size(13.0),
                );
                ui.label(
                    RichText::new(label)
                        .color(fg)
                        .size(12.0)
                        .strong(),
                );
            });
        });
}

/// A primary "filled" button with rounded corners.
fn primary_button(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let btn = egui::Button::new(
        RichText::new(text)
            .color(if enabled {
                Color32::WHITE
            } else {
                Color32::from_rgb(148, 163, 184)
            })
            .strong()
            .size(13.0),
    )
    .fill(if enabled {
        theme::ACCENT
    } else {
        Color32::from_rgb(226, 232, 240)
    })
    .stroke(Stroke::NONE)
    .corner_radius(CornerRadius::same(8))
    .min_size(Vec2::new(0.0, 32.0));

    ui.add_enabled(enabled, btn)
}

/// A "ghost" / outline button.
fn ghost_button(ui: &mut Ui, text: &str) -> Response {
    let btn = egui::Button::new(
        RichText::new(text)
            .color(theme::TEXT_PRIMARY)
            .size(13.0),
    )
    .fill(Color32::TRANSPARENT)
    .stroke(Stroke::new(1.0, theme::CARD_BORDER))
    .corner_radius(CornerRadius::same(8))
    .min_size(Vec2::new(0.0, 32.0));
    ui.add(btn)
}

// ---------------------------------------------------------------------------
// eframe::App implementation
// ---------------------------------------------------------------------------

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_workers();

        // Accept dropped files. egui hands us a `DroppedFile` with an
        // optional `path: PathBuf` and a `name: String`. We only care
        // about the path; the name is shown to the user but is not
        // usable as an on-disk path on Linux.
        let dropped: Vec<PathBuf> = ui.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        for p in dropped {
            self.add_path(p);
        }

        let dragging = is_being_dragged(&ctx);

        // --- Outer page frame: 24px padding, soft page background.
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(theme::PAGE_BG)
                    .inner_margin(Margin::same(24)),
            )
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());

                    // --- Header: brand + subtitle.
                    self.draw_header(ui);
                    ui.add_space(20.0);

                    // --- Drop zone / file picker card.
                    self.draw_drop_zone(ui, dragging);
                    ui.add_space(20.0);

                    // --- File list (only shown when at least one file
                    //     has been added; otherwise the drop zone is
                    //     enough).
                    if !self.files.is_empty() {
                        self.draw_file_list(ui, &ctx);
                        ui.add_space(16.0);
                    }

                    // --- Footer.
                    self.draw_footer(ui);
                });
            });
    }
}

impl App {
    fn draw_header(&self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            // Brand mark: a small accent square.
            let (rect, _response) = ui.allocate_exact_size(Vec2::new(28.0, 28.0), Sense::hover());
            ui.painter().rect_filled(rect, CornerRadius::same(7), theme::ACCENT);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "📄",
                egui::FontId::proportional(16.0),
                Color32::WHITE,
            );
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(2.0, 2.0);
                ui.label(
                    RichText::new("caj2pdf")
                        .strong()
                        .size(20.0)
                        .color(theme::TEXT_PRIMARY),
                );
                ui.label(
                    RichText::new("知网 CAJ / HN / C8 / KDH → PDF")
                        .size(12.0)
                        .color(theme::TEXT_SECONDARY),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                        .monospace()
                        .size(11.0)
                        .color(theme::TEXT_SECONDARY),
                );
            });
        });
    }

    fn draw_drop_zone(&mut self, ui: &mut Ui, dragging: bool) {
        // Whole card is a drop target: allocate the size with a sense
        // that allows drops, so the user can drop anywhere on it.
        let (bg, border, border_w, label_main, label_sub) = if dragging {
            (
                theme::DRAG_BG,
                theme::DRAG_BORDER_STRONG,
                2.0,
                "松开以添加文件".to_owned(),
                "检测到拖入…".to_owned(),
            )
        } else {
            (
                theme::CARD_BG,
                theme::CARD_BORDER,
                1.0,
                "拖入文件或点击添加".to_owned(),
                "支持 .caj / .hn / .c8 / .kdh / .pdf".to_owned(),
            )
        };

        let card_resp = card(ui, bg, border, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 16.0;
                    // Big icon: a stack of papers.
                    let (icon_rect, _) =
                        ui.allocate_exact_size(Vec2::new(48.0, 48.0), Sense::hover());
                    ui.painter().rect_filled(
                        icon_rect,
                        CornerRadius::same(12),
                        if dragging {
                            theme::ACCENT
                        } else {
                            Color32::from_rgb(241, 245, 249)
                        },
                    );
                    ui.painter().text(
                        icon_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        if dragging { "📥" } else { "📂" },
                        egui::FontId::proportional(24.0),
                        if dragging {
                            Color32::WHITE
                        } else {
                            theme::TEXT_SECONDARY
                        },
                    );

                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(2.0, 2.0);
                        ui.label(
                            RichText::new(label_main)
                                .strong()
                                .size(15.0)
                                .color(theme::TEXT_PRIMARY),
                        );
                        ui.label(
                            RichText::new(label_sub)
                                .size(12.0)
                                .color(theme::TEXT_SECONDARY),
                        );
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            if ghost_button(ui, "选择文件").clicked() {
                                if let Some(paths) = rfd::FileDialog::new()
                                    .add_filter(
                                        "CAJ family",
                                        &["caj", "hn", "c8", "kdh", "pdf", "teb"],
                                    )
                                    .pick_files()
                                {
                                    for p in paths {
                                        self.add_path(p);
                                    }
                                }
                            }
                            let pending = self
                                .files
                                .iter()
                                .filter(|e| matches!(e.status, Status::Pending | Status::Failed(_)))
                                .count();
                            let can_convert = pending > 0;
                            let label = if pending == 0 && !self.files.is_empty() {
                                "全部完成".to_owned()
                            } else if pending == self.files.len() {
                                format!("开始转换 ({} 个)", pending)
                            } else {
                                format!("重试 ({} 个)", pending)
                            };
                            if primary_button(ui, &label, can_convert).clicked() {
                                self.start_conversion(ui.ctx().clone());
                            }
                            if !self.files.is_empty() {
                                if ghost_button(ui, "清空列表").clicked() {
                                    self.files.clear();
                                }
                            }
                        });
                    });
                });
            });
        });

        // Use the full card area as a drop target.
        let _ = card_resp.interact(Sense::hover()).on_hover_cursor(CursorIcon::PointingHand);

        // Visual emphasis when dragging.
        if dragging {
            let painter = ui.painter_at(card_resp.rect);
            painter.rect_stroke(
                card_resp.rect,
                CornerRadius::same(12),
                Stroke::new(border_w, theme::DRAG_BORDER_STRONG),
                StrokeKind::Inside,
            );
        }
    }

    fn draw_file_list(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let _ = ctx; // silence unused

        // Section title.
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("文件列表")
                    .strong()
                    .size(13.0)
                    .color(theme::TEXT_PRIMARY),
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!("{} 个", self.files.len()))
                    .color(theme::TEXT_SECONDARY)
                    .size(12.0),
            );
        });
        ui.add_space(8.0);

        // Each file = one card row.
        let mut to_remove: Vec<usize> = Vec::new();
        for (idx, entry) in self.files.iter().enumerate() {
            let is_done = matches!(entry.status, Status::Done(_));
            let card_resp = card(
                ui,
                if is_done {
                    theme::CARD_BG
                } else {
                    theme::CARD_BG
                },
                theme::CARD_BORDER,
                |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        ui.set_min_height(28.0);

                        // Status icon column (fixed width).
                        let (icon_rect, _) =
                            ui.allocate_exact_size(Vec2::new(20.0, 20.0), Sense::hover());
                        let icon_color = match &entry.status {
                            Status::Pending => theme::STATUS_PENDING_FG,
                            Status::Running => theme::STATUS_RUNNING_FG,
                            Status::Done(_) => theme::STATUS_DONE_FG,
                            Status::Failed(_) => theme::STATUS_ERROR_FG,
                        };
                        ui.painter().text(
                            icon_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            entry.status.icon(),
                            egui::FontId::proportional(15.0),
                            icon_color,
                        );

                        // File name + optional output line.
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(1.0, 1.0);
                            ui.label(
                                RichText::new(display_name(&entry.input))
                                    .strong()
                                    .size(13.0)
                                    .color(theme::TEXT_PRIMARY),
                            );
                            // Show output path or error inline.
                            let sub = match &entry.status {
                                Status::Done(out) => {
                                    format!("→ {}", out.display())
                                }
                                Status::Failed(e) => {
                                    format!("错误: {}", e)
                                }
                                _ => display_dir(&entry.input),
                            };
                            let sub_color = match &entry.status {
                                Status::Done(_) | Status::Pending | Status::Running => {
                                    theme::TEXT_SECONDARY
                                }
                                Status::Failed(_) => theme::STATUS_ERROR_FG,
                            };
                            ui.label(
                                RichText::new(sub)
                                    .size(11.0)
                                    .color(sub_color),
                            );
                        });

                        // Spacer pushes the badge + remove to the right.
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("✕")
                                            .size(13.0)
                                            .color(theme::TEXT_SECONDARY),
                                    )
                                    .fill(Color32::TRANSPARENT)
                                    .stroke(Stroke::NONE)
                                    .min_size(Vec2::new(28.0, 28.0)),
                                )
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .clicked()
                            {
                                to_remove.push(idx);
                            }
                            status_badge(ui, &entry.status);
                        });
                    });
                },
            );
            // Subtle hover highlight: re-fill with a slightly darker bg
            // if hovered. We do this by just relying on the system
            // cursor; a real impl could re-paint.
            let _ = card_resp.on_hover_cursor(CursorIcon::Default);
            ui.add_space(6.0);
        }
        for &i in to_remove.iter().rev() {
            self.files.remove(i);
        }
    }

    fn draw_footer(&self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                ui.label(
                    RichText::new("caj2pdf-rs · 纯 Rust · MIT/Apache-2.0")
                        .size(10.0)
                        .color(theme::TEXT_SECONDARY),
                );
            });
        });
    }
}

// ---------------------------------------------------------------------------
// Path display helpers
// ---------------------------------------------------------------------------

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn display_dir(path: &Path) -> String {
    path.parent()
        .map(|p| p.display().to_string())
        .unwrap_or_default()
}
