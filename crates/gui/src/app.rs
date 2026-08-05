//! Stub `App` implementation. Real drag-drop, worker pool, and grid
//! view land in Patch 3.

#![allow(dead_code)]

use std::path::PathBuf;

use eframe::egui;

#[derive(Debug, Default)]
pub struct App {
    pub files: Vec<FileEntry>,
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub input: PathBuf,
    pub status: Status,
}

#[derive(Debug, Clone)]
pub enum Status {
    Pending,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.label("caj2pdf — Patch 2 stub. Real UI lands in Patch 3.");
        for f in &self.files {
            ui.label(format!("{:?}", f.input));
        }
    }
}
