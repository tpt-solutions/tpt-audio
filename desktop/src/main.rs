use std::sync::Arc;

use tpt_audio_core::backend::AudioBackend;
use tpt_audio_core::diagnostics::Diagnostics;

type BackendResult = Result<(Box<dyn AudioBackend>, Arc<Diagnostics>), Box<dyn std::error::Error>>;

#[cfg(target_os = "windows")]
fn build_backend() -> BackendResult {
    let backend = tpt_audio_platform_windows::WasapiBackend::new()?;
    let diagnostics = backend.diagnostics();
    Ok((Box::new(backend), diagnostics))
}

#[cfg(target_os = "linux")]
fn build_backend() -> BackendResult {
    let backend = tpt_audio_platform_linux::PipewireBackend::new()?;
    let diagnostics = backend.diagnostics();
    Ok((Box::new(backend), diagnostics))
}

#[cfg(all(
    feature = "archon",
    not(target_os = "windows"),
    not(target_os = "linux")
))]
fn build_backend() -> BackendResult {
    let backend = tpt_audio_platform_archon::ArchonBackend::new()?;
    let diagnostics = backend.diagnostics();
    Ok((Box::new(backend), diagnostics))
}

#[cfg(not(any(
    target_os = "windows",
    target_os = "linux",
    all(
        feature = "archon",
        not(target_os = "windows"),
        not(target_os = "linux")
    )
)))]
fn build_backend() -> BackendResult {
    Err("No audio backend configured for this target".into())
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if let Some(idx) = args.iter().position(|a| a == "--control") {
        let command = args
            .get(idx + 1)
            .map(|s| s.as_str())
            .unwrap_or_else(|| {
                eprintln!("tpt-audio: --control requires a JSON command argument");
                std::process::exit(2);
            });
        return run_control(command);
    }

    if args.iter().any(|a| a == "--server") {
        return run_server();
    }

    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        return Ok(());
    }

    let (backend, diagnostics) = build_backend().expect("Failed to initialize audio backend");

    install_crash_reporting(diagnostics.clone());
    let app = tpt_audio_gui::App::new(backend, diagnostics);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([900.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native("tpt-audio", options, Box::new(|_cc| Ok(Box::new(app))))
}

/// Headless one-shot control: run a single JSON command and print its response.
fn run_control(command: &str) -> eframe::Result<()> {
    let (mut backend, _diagnostics) = match build_backend() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("tpt-audio: failed to initialize audio backend: {e}");
            std::process::exit(1);
        }
    };
    let out = tpt_audio_core::remote::dispatch(backend.as_mut(), command);
    println!("{out}");
    Ok(())
}

/// Headless server: read newline-delimited JSON commands from stdin and print
/// JSON responses to stdout until EOF.
fn run_server() -> eframe::Result<()> {
    let (mut backend, _diagnostics) = match build_backend() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("tpt-audio: failed to initialize audio backend: {e}");
            std::process::exit(1);
        }
    };
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    tpt_audio_core::remote::run_stdin_server(backend.as_mut(), stdin.lock(), &mut stdout);
    Ok(())
}

fn print_usage() {
    println!(
        "tpt-audio — Audio Router & Virtual Mixer\n\
\n\
Usage:\n\
  tpt-audio                 Launch the graphical interface\n\
  tpt-audio --control JSON  Run a single remote command (headless) and print JSON\n\
  tpt-audio --server        Read newline-delimited JSON commands from stdin\n\
  tpt-audio --help          Show this help\n\
\n\
Example:\n\
  tpt-audio --control '{{\"action\":\"enumerate_devices\"}}'"
    );
}

/// Persist a rolling diagnostics log and install a panic hook that dumps a crash
/// report to the temp directory on unexpected termination.
fn install_crash_reporting(diagnostics: std::sync::Arc<tpt_audio_core::diagnostics::Diagnostics>) {
    let mut log_path = std::env::temp_dir();
    log_path.push(format!("tpt-audio-{}.log", std::process::id()));
    if let Err(e) = diagnostics.set_log_file(&log_path) {
        eprintln!("tpt-audio: could not open diagnostics log file: {e}");
    }

    std::panic::set_hook(Box::new(move |info| {
        let msg = info.to_string();
        diagnostics.error(format!("panic: {msg}"));
        if let Some(path) = diagnostics.write_crash_report(&msg) {
            eprintln!("tpt-audio crashed. Report written to {path:?}");
        }
    }));
}
