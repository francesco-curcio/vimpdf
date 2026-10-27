mod app;
mod config;
mod document;
mod input;
mod jumplist;
mod layout;
mod links;
mod modes;
mod render;
mod search;
mod text;
mod ui;
mod view;
mod viewport;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::Parser;
use tracing_subscriber::EnvFilter;

use crate::app::PdfvApp;
use crate::config::Config;
use crate::document::DocInfo;

/// A keyboard-driven, vim-style PDF viewer.
#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// PDF file to open.
    file: PathBuf,

    /// Config file to use instead of ~/.config/pdfv/config.toml.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("pdfv=info")),
        )
        .init();

    let args = Args::parse();
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("pdfv: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Args) -> Result<()> {
    let path = std::fs::canonicalize(&args.file)
        .with_context(|| format!("cannot open '{}'", args.file.display()))?;
    if !path.is_file() {
        bail!("'{}' is not a file", path.display());
    }

    let info = DocInfo::load(&path)?;

    let config_path = args.config.or_else(Config::default_path);
    let (config, warnings) = match &config_path {
        Some(p) => Config::load(p),
        None => (Config::default(), Vec::new()),
    };

    let title = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "pdfv".to_owned());

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(&title)
            .with_inner_size([900.0, 1100.0])
            .with_maximized(config.start_maximised)
            .with_active(true),
        ..Default::default()
    };

    eframe::run_native(
        "pdfv",
        options,
        Box::new(move |cc| {
            Ok(Box::new(PdfvApp::new(
                cc,
                path,
                info,
                config,
                config_path,
                warnings,
            )))
        }),
    )
    .map_err(|e| anyhow::anyhow!("failed to start window: {e}"))
}
