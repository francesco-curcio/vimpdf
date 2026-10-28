mod annotate;
mod app;
mod config;
mod document;
mod input;
mod jumplist;
mod layout;
mod links;
mod modes;
mod platform;
mod present;
mod render;
mod search;
mod selection;
mod state;
mod text;
mod ui;
mod view;
mod viewport;
mod watch;
mod welcome;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::Parser;
use tracing_subscriber::EnvFilter;

use crate::app::VimpdfApp;
use crate::config::Config;
use crate::document::DocInfo;

/// A keyboard-driven, vim-style PDF viewer.
#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// PDF file to open (without one, vimpdf waits for a file from the system,
    /// e.g. Finder's Open With, or shows a welcome page).
    file: Option<PathBuf>,

    /// Config file to use instead of ~/.config/vimpdf/config.toml.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("vimpdf=info")),
        )
        .init();

    let args = Args::parse();
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("vimpdf: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Args) -> Result<()> {
    // Working copies from sessions that didn't quit cleanly.
    annotate::remove_stale_work_dirs(std::time::Duration::from_secs(24 * 3600));

    // A file given on the command line is checked before any window opens,
    // so mistakes are reported in the terminal.
    let initial = match &args.file {
        Some(file) => {
            let path = std::fs::canonicalize(file)
                .with_context(|| format!("cannot open '{}'", file.display()))?;
            if !path.is_file() {
                bail!("'{}' is not a file", path.display());
            }
            let info = DocInfo::load(&path)?;
            Some((path, info))
        }
        None => None,
    };

    let config_path = args.config.or_else(Config::default_path);
    let (config, warnings) = match &config_path {
        Some(p) => Config::load(p),
        None => (Config::default(), Vec::new()),
    };

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("vimpdf")
            // Wayland app id / X11 class: matches vimpdf.desktop, for the icon.
            .with_app_id("vimpdf")
            .with_inner_size([900.0, 1100.0])
            .with_maximized(config.start_maximised)
            .with_active(true),
        wgpu_options: eframe::WgpuConfiguration::default().with_surface_config(present::initial()),
        ..Default::default()
    };

    // We create the event loop ourselves (instead of eframe::run_native) so
    // the macOS open-file handler can be installed between its creation and
    // its start: files opened from Finder arrive while it starts.
    let event_loop = winit::event_loop::EventLoop::<eframe::UserEvent>::with_user_event()
        .build()
        .context("cannot create the event loop")?;
    platform::install_open_handler();

    let creator: eframe::AppCreator<'_> = Box::new(move |cc| {
        let ctx = cc.egui_ctx.clone();
        platform::set_waker(move || ctx.request_repaint());
        let mut warnings = warnings;
        let (path, info) = match initial {
            Some(doc) => doc,
            None => initial_document(&mut warnings)?,
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        cc.egui_ctx
            .send_viewport_cmd(eframe::egui::ViewportCommand::Title(name));
        let present = present::preferred(cc);
        Ok(Box::new(
            VimpdfApp::new(cc, path, info, config, config_path, warnings).with_present(present),
        ))
    });
    let mut app = eframe::create_native("vimpdf", options, creator, &event_loop);
    event_loop
        .run_app(&mut app)
        .map_err(|e| anyhow::anyhow!("failed to run the window: {e}"))
}

/// Without a command-line file: the last file the system asked us to open
/// while launching, else the welcome page.
fn initial_document(
    warnings: &mut Vec<String>,
) -> Result<(PathBuf, DocInfo), Box<dyn std::error::Error + Send + Sync>> {
    for path in platform::take_opened().into_iter().rev() {
        match std::fs::canonicalize(&path)
            .map_err(anyhow::Error::from)
            .and_then(|p| DocInfo::load(&p).map(|info| (p, info)))
        {
            Ok(doc) => return Ok(doc),
            Err(e) => warnings.push(format!("cannot open {}: {e:#}", path.display())),
        }
    }
    let path = welcome::write()?;
    let info = DocInfo::load(&path)?;
    Ok((path, info))
}
