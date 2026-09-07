use anyhow::{Context, Result, bail};
use crossterm::{
    cursor::{Hide, Show},
    event::{self, DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use rec::{config::Config, session::Transport, ui::App};
use std::{
    env,
    ffi::OsString,
    io::{self, IsTerminal},
    path::PathBuf,
    time::Duration,
};

const HELP: &str = "rec — standalone multichannel recorder/player (foundation)\n\nUsage: rec [--data-root PATH] [--init-config]\n       rec --help | --version\n\n--data-root PATH  Private configuration and future takes\n--init-config     Create default config.json without replacing existing files\n\nRoot: --data-root, then REC_DATA_ROOT, then XDG_DATA_HOME/rec,\n      then HOME/.local/share/rec.\nTouch: PREV/NEXT, ARM, STOP, EXIT. Keyboard: arrows, Space, s, q/Esc.\nAudio recording/playback and MIDI are not implemented in this foundation.\n";

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rec: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let mut root = None;
    let mut init = false;
    let mut args = env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--help" | "-h") => {
                print!("{HELP}");
                return Ok(());
            }
            Some("--version" | "-V") => {
                println!("rec {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            Some("--data-root") => {
                let value = args.next().context("--data-root requires a path")?;
                if value.is_empty() {
                    bail!("--data-root requires a nonempty path");
                }
                if root.replace(PathBuf::from(value)).is_some() {
                    bail!("--data-root provided twice");
                }
            }
            Some("--init-config") => init = true,
            _ => bail!("unknown argument {:?}; use --help", arg),
        }
    }
    let root = match root {
        Some(root) => root,
        None => default_root()?,
    };
    if init {
        Config::create_default(&root)
            .with_context(|| format!("create config in {}", root.display()))?;
        println!("Created {}", root.join("config.json").display());
        return Ok(());
    }
    let config = Config::load(&root)?;
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("interactive terminal required; use --help");
    }
    let mut app = App::new(config);
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    for signal in [
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGINT,
        signal_hook::consts::SIGHUP,
    ] {
        signal_hook::flag::register(signal, shutdown.clone())?;
    }
    let _guard = TerminalGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;
    while app.session.transport() != Transport::Exited {
        if shutdown.load(std::sync::atomic::Ordering::Relaxed) {
            // This milestone has no engine resources. Future adapters must drive
            // the quiescence/finalization protocol here before returning.
            app.session.request_exit();
            if app.session.transport() == Transport::Exited {
                break;
            }
        }
        terminal.draw(|frame| app.draw(frame))?;
        if event::poll(Duration::from_millis(100))?
            && let Some(action) = app.event(event::read()?, terminal.size()?)
        {
            app.dispatch(action);
        }
    }
    Ok(())
}

fn nonempty_env(name: &str) -> Option<OsString> {
    env::var_os(name).filter(|value| !value.is_empty())
}
fn default_root() -> Result<PathBuf> {
    if let Some(root) = nonempty_env("REC_DATA_ROOT") {
        return Ok(root.into());
    }
    if let Some(root) = nonempty_env("XDG_DATA_HOME") {
        let root = PathBuf::from(root);
        if !root.is_absolute() {
            bail!("XDG_DATA_HOME must be absolute");
        }
        return Ok(root.join("rec"));
    }
    let home = nonempty_env("HOME")
        .context("set --data-root or REC_DATA_ROOT when HOME is unavailable")?;
    Ok(PathBuf::from(home).join(".local/share/rec"))
}

struct TerminalGuard;
impl TerminalGuard {
    fn enter() -> Result<Self> {
        // Construct before the first mutation so partial initialization unwinds.
        let guard = Self;
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture, Hide)?;
        Ok(guard)
    }
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // Attempt every restoration even if an earlier terminal write fails.
        let _ = execute!(io::stdout(), DisableMouseCapture);
        let _ = execute!(io::stdout(), Show);
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}
