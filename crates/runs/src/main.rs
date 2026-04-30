use std::time::Duration;
#[cfg(unix)]
use std::path::PathBuf;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{Shell, generate};
use tracing_appender::non_blocking::WorkerGuard;
use crossterm::{
    event::{Event, EventStream},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use futures_util::StreamExt;
use ratatui::{Terminal, backend::CrosstermBackend};
use tokio::sync::mpsc;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use runs::{
    app::{
        msg::AppMsg,
        state::App,
        update::{Effect, update},
    },
    config::RunsConfig,
    network::spawn as spawn_network,
    ui,
};

#[derive(Parser, Debug)]
#[command(name = "runs", about = "TUI client for runsd")]
struct Cli {
    /// Path to the runsd Unix socket (Unix only).
    /// Defaults to $XDG_RUNTIME_DIR/runsd.sock, falling back to /tmp/runsd.sock.
    #[cfg(unix)]
    #[arg(long, env = "RUNSD_SOCKET")]
    socket: Option<PathBuf>,

    /// TCP port of the runsd server (Windows only)
    #[cfg(windows)]
    #[arg(long, default_value_t = 4242, env = "RUNSD_PORT")]
    port: u16,

    #[command(subcommand)]
    command: Option<Command>,

    /// Print shell completions for the given shell and exit.
    #[arg(long, value_name = "SHELL")]
    completions: Option<Shell>,

    /// Print the default runs config as TOML and exit.
    #[arg(long)]
    init_config: bool,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Check connectivity and daemon health, then exit.
    Doctor,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialise tracing: stderr (quiet) + rolling log file under XDG_STATE_HOME.
    let _log_guard = setup_tracing();

    // Restore the terminal on panic so the shell isn't left in raw mode.
    install_panic_hook();

    let cli = Cli::parse();

    if let Some(shell) = cli.completions {
        generate(shell, &mut Cli::command(), "runs", &mut std::io::stdout());
        return Ok(());
    }

    if cli.init_config {
        let default = RunsConfig::default();
        print!("{}", toml::to_string_pretty(&default).expect("serialize config"));
        return Ok(());
    }

    if let Some(Command::Doctor) = cli.command {
        return run_doctor(&cli).await;
    }

    let cfg = RunsConfig::load();

    // App message channel — all inputs flow here.
    let (app_tx, mut app_rx) = mpsc::channel::<AppMsg>(256);

    // Spawn network task.
    #[cfg(unix)]
    let net_tx = {
        let socket = cli.socket
            .or_else(|| cfg.socket_path.clone())
            .unwrap_or_else(|| {
                std::env::var("XDG_RUNTIME_DIR")
                    .map(|p| PathBuf::from(p).join("runsd.sock"))
                    .unwrap_or_else(|_| std::env::temp_dir().join("runsd.sock"))
            });
        spawn_network(socket, app_tx.clone(), cfg.page_size)
    };
    #[cfg(windows)]
    let net_tx = spawn_network(cli.port, app_tx.clone(), cfg.page_size);

    // Set up terminal.
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    // Input task — reads crossterm events and forwards to app_tx.
    let input_tx = app_tx.clone();
    tokio::spawn(async move {
        let mut stream = EventStream::new();
        while let Some(Ok(event)) = stream.next().await {
            match event {
                Event::Key(key) => {
                    let _ = input_tx.send(AppMsg::Key(key)).await;
                }
                Event::Resize(w, h) => {
                    let _ = input_tx.send(AppMsg::Resize(w, h)).await;
                }
                _ => {}
            }
        }
    });

    // Render loop — event-driven with 16ms debounce.
    let mut app = App::new();
    let debounce = Duration::from_millis(16);

    loop {
        // Drain one or more messages, then render.
        // When no message arrives within the debounce window, fire a Tick so
        // the spinner and other frame-rate-dependent UI can advance.
        let msg = tokio::time::timeout(debounce, app_rx.recv())
            .await
            .ok()
            .flatten()
            .or(Some(AppMsg::Tick));

        if let Some(msg) = msg {
            let (new_app, effects) = update(app, msg);
            app = new_app;

            for effect in effects {
                match effect {
                    Effect::Quit => {
                        restore_terminal(&mut terminal)?;
                        return Ok(());
                    }
                    Effect::Network(cmd) => {
                        let _ = net_tx.send(cmd).await;
                    }
                    Effect::SaveResult { calc_id } => {
                        let _ = app_tx
                            .send(AppMsg::CmdOk(format!("Result saved for {calc_id}")))
                            .await;
                    }
                    Effect::Yank(text) => {
                        yank_to_clipboard(&text);
                    }
                }
            }
        }

        // Redraw.
        terminal.draw(|f| ui::render(f, &app))?;
    }
}

/// Copy `text` to the system clipboard using platform-specific tools.
fn yank_to_clipboard(text: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(stdin) = child.stdin.as_mut() {
                    stdin.write_all(text.as_bytes())?;
                }
                child.wait()
            });
    }
    #[cfg(target_os = "linux")]
    {
        // Try xclip first, then xsel.
        let ok = std::process::Command::new("xclip")
            .args(["-selection", "clipboard"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(stdin) = child.stdin.as_mut() {
                    stdin.write_all(text.as_bytes())?;
                }
                child.wait()
            })
            .is_ok();
        if !ok {
            let _ = std::process::Command::new("xsel")
                .args(["--clipboard", "--input"])
                .stdin(std::process::Stdio::piped())
                .spawn()
                .and_then(|mut child| {
                    use std::io::Write;
                    if let Some(stdin) = child.stdin.as_mut() {
                        stdin.write_all(text.as_bytes())?;
                    }
                    child.wait()
                });
        }
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("clip")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(stdin) = child.stdin.as_mut() {
                    stdin.write_all(text.as_bytes())?;
                }
                child.wait()
            });
    }
}

async fn run_doctor(cli: &Cli) -> anyhow::Result<()> {
    use runs::network::Client;

    let mut all_ok = true;
    let print = |ok: bool, label: &str, detail: &str| {
        let mark = if ok { "✓" } else { "✗" };
        println!("{mark} {label}: {detail}");
    };

    #[cfg(unix)]
    let socket = {
        let path = cli.socket.clone().unwrap_or_else(|| {
            std::env::var("XDG_RUNTIME_DIR")
                .map(|p| std::path::PathBuf::from(p).join("runsd.sock"))
                .unwrap_or_else(|_| std::env::temp_dir().join("runsd.sock"))
        });

        let exists = path.exists();
        print(exists, "socket", &path.display().to_string());
        if !exists {
            all_ok = false;
        }
        path
    };

    #[cfg(windows)]
    let port = cli.port;

    // Try connecting and calling /healthz.
    #[cfg(unix)]
    let client = Client::new(socket);
    #[cfg(windows)]
    let client = Client::new(port);

    match client.get("/healthz").await {
        Ok((status, body)) => {
            let ok = status.is_success();
            all_ok &= ok;
            let body_str = String::from_utf8_lossy(&body);
            print(ok, "healthz", &format!("HTTP {status} — {body_str}"));
        }
        Err(e) => {
            all_ok = false;
            print(false, "healthz", &format!("connect failed: {e}"));
        }
    }

    if all_ok {
        println!("\nAll checks passed.");
        std::process::exit(0);
    } else {
        println!("\nSome checks failed — is runsd running?");
        std::process::exit(1);
    }
}

fn setup_tracing() -> WorkerGuard {
    use tracing_subscriber::Layer;

    let log_dir = std::env::var("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var("HOME")
                .map(|h| std::path::PathBuf::from(h).join(".local/state"))
                .unwrap_or_else(|_| std::env::temp_dir())
        })
        .join("runs");
    let _ = std::fs::create_dir_all(&log_dir);

    let file_appender = tracing_appender::rolling::RollingFileAppender::builder()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("runs")
        .filename_suffix("log")
        .max_log_files(7)
        .build(&log_dir)
        .expect("build file appender");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let stderr_filter = EnvFilter::from_default_env();
    let file_filter = EnvFilter::new("debug");

    tracing_subscriber::registry()
        .with(fmt::layer().with_writer(std::io::stderr).with_filter(stderr_filter))
        .with(fmt::layer().json().with_writer(non_blocking).with_filter(file_filter))
        .init();

    guard
}

/// Install a panic hook that restores the terminal before printing the panic message.
/// Without this, a panic leaves the shell in raw mode with the alternate screen active.
fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::terminal::LeaveAlternateScreen,
        );
        original(info);
    }));
}

fn restore_terminal(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
) -> anyhow::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}
