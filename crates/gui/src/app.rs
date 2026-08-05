//! `App` state + the `eframe::App` implementation.
//!
//! Threading model: the `core_convert` function is CPU-bound (JBIG
//! decode + PDF assembly), so we never call it on the UI thread. The
//! "Convert all" button spawns one `std::thread` per pending file;
//! each worker pushes a [`WorkerMsg`] back through the channel. The
//! UI calls [`App::poll_workers`] at the start of every frame to
//! drain the channel and update statuses.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread::{self, JoinHandle};

use anyhow::{Context, Result};
use eframe::egui;

use caj2pdf_core::convert::convert as core_convert;

// ---------------------------------------------------------------------------
// Public data model
// ---------------------------------------------------------------------------

/// One row in the file list.
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
    fn label(&self) -> &'static str {
        match self {
            Status::Pending => "pending",
            Status::Running => "running",
            Status::Done(_) => "done",
            Status::Failed(_) => "error",
        }
    }
}

// ---------------------------------------------------------------------------
// Worker pool plumbing
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum WorkerMsg {
    Started(PathBuf),
    Finished { input: PathBuf, result: Result<PathBuf, String> },
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
        let Some(rx) = &self.rx else { return };
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
                let result = convert_one(&entry.input)
                    .map_err(|e| format!("{:#}", e));
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

        ui.heading("caj2pdf");
        ui.label(
            "Drop CAJ / HN / C8 / KDH / PDF files here, or use the Add button. \
             Then click Convert all.",
        );

        ui.horizontal(|ui| {
            if ui.button("Add files…").clicked() {
                if let Some(paths) = rfd::FileDialog::new()
                    .add_filter("CAJ family", &["caj", "hn", "c8", "kdh", "pdf", "teb"])
                    .pick_files()
                {
                    for p in paths {
                        self.add_path(p);
                    }
                }
            }
            let can_convert = self
                .files
                .iter()
                .any(|e| matches!(e.status, Status::Pending | Status::Failed(_)));
            if ui
                .add_enabled(can_convert, egui::Button::new("Convert all"))
                .clicked()
            {
                self.start_conversion(ctx.clone());
            }
            if ui.button("Clear").clicked() {
                self.files.clear();
            }
        });

        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("files")
                .striped(true)
                .num_columns(4)
                .spacing([12.0, 4.0])
                .show(ui, |ui| {
                    ui.strong("Input");
                    ui.strong("Status");
                    ui.strong("Output / Error");
                    ui.strong("");
                    ui.end_row();

                    let mut to_remove: Vec<usize> = Vec::new();
                    for (idx, entry) in self.files.iter().enumerate() {
                        ui.label(entry.input.display().to_string());
                        ui.label(entry.status.label());
                        match &entry.status {
                            Status::Done(p) => {
                                ui.label(p.display().to_string());
                            }
                            Status::Failed(e) => {
                                ui.label(
                                    egui::RichText::new(e)
                                        .color(egui::Color32::from_rgb(220, 80, 80)),
                                );
                            }
                            _ => {
                                ui.label("");
                            }
                        }
                        if ui.button("✕").clicked() {
                            to_remove.push(idx);
                        }
                        ui.end_row();
                    }
                    for &i in to_remove.iter().rev() {
                        self.files.remove(i);
                    }
                });
        });

        ui.separator();
        ui.collapsing("About", |ui| {
            ui.label(format!(
                "caj2pdf-rs {} — pure-Rust CAJ/HN/KDH to PDF",
                env!("CARGO_PKG_VERSION")
            ));
            ui.hyperlink("https://github.com/duststarr/caj2pdf-rs");
        });
    }
}

// Suppress dead-code warnings on the sender side.
#[allow(dead_code)]
fn _suppress_unused_tx(_: &Sender<WorkerMsg>) {}
