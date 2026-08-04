//! `caj2pdf` CLI entry point.
//!
//! Subcommands mirror the original Python tool:
//!
//! * `show`      — print the file's format, page count, and outline count
//! * `convert`   — convert a CAJ/HN/C8 file to a PDF
//! * `outlines`  — add outline entries from a CAJ/HN/C8 file to an
//!                 existing PDF (printed from CAJViewer, for example)
//! * `text-extract` — dump the extracted text of every page
//! * `parse`     — debug dump of every page's structure

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "caj2pdf",
    about = "Convert Chinese academic journal (CAJ/HN) files to PDF",
    long_about = None,
    version,
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show the file's format, page count, and outline count.
    Show {
        /// Path to the input CAJ-family file.
        input: String,
    },
    /// Convert a CAJ/HN/C8 file to PDF.
    Convert {
        /// Path to the input CAJ-family file.
        input: String,
        /// Path to the output PDF (default: replace extension with .pdf).
        #[arg(short = 'o', long = "output")]
        output: Option<String>,
    },
    /// Inject the outline tree of a CAJ/HN/C8 file into an existing PDF.
    Outlines {
        /// Path to the input CAJ-family file.
        input: String,
        /// Path to the existing PDF.
        #[arg(short = 'o', long = "output")]
        output: String,
    },
    /// Dump the extracted text of every page.
    TextExtract {
        /// Path to the input CAJ-family file.
        input: String,
    },
    /// Debug-dump every page's structure.
    Parse {
        /// Path to the input CAJ-family file.
        input: String,
    },
}

fn main() -> Result<()> {
    init_tracing();
    let cli = Cli::parse();
    match cli.command {
        Command::Show { input } => cmd_show(&input).context("show"),
        Command::Convert { input, output } => cmd_convert(&input, output.as_deref()).context("convert"),
        Command::Outlines { input, output } => cmd_outlines(&input, &output).context("outlines"),
        Command::TextExtract { input } => cmd_text_extract(&input).context("text-extract"),
        Command::Parse { input } => cmd_parse(&input).context("parse"),
    }
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}

fn cmd_show(input: &str) -> Result<()> {
    unimplemented!("filled in by integration agent")
}

fn cmd_convert(input: &str, output: Option<&str>) -> Result<()> {
    unimplemented!("filled in by integration agent")
}

fn cmd_outlines(input: &str, output: &str) -> Result<()> {
    unimplemented!("filled in by integration agent")
}

fn cmd_text_extract(input: &str) -> Result<()> {
    unimplemented!("filled in by integration agent")
}

fn cmd_parse(input: &str) -> Result<()> {
    unimplemented!("filled in by integration agent")
}
