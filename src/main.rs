use anyhow::{Context, Result, bail};
use crossterm::{
    cursor::{Hide, Show},
    event::{self, DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use shr_rec::{config::Config, session::Transport, ui::App};
use std::{
    env,
    ffi::OsString,
    io::{self, IsTerminal},
    path::PathBuf,
    time::Duration,
};

const HELP: &str = "shr-rec — standalone multichannel take player / recorder shell\n\nUsage: shr-rec [--data-root PATH] [--takes-root PATH] [--open TAKE]\n       shr-rec [--data-root PATH] --init-config\n       shr-rec --help | --version\n\n--data-root PATH  Private configuration (REC_DATA_ROOT also supported)\n--takes-root PATH Read-only take library (default DATA_ROOT/takes)\n--open TAKE       Open one native or SHR format-1 take directory\n--init-config     Create config.json without replacing existing files\n\nRoot: --data-root, REC_DATA_ROOT, XDG_DATA_HOME/rec, HOME/.local/share/rec.\nTouch TAKES, OPEN, OUTPUTS; choose exact or stereo routes, then PLAY.\nKeyboard: arrows, Enter, p/Space, s, q; 1–8 activate visible controls.\nJACK attaches only on explicit output discovery/play; never starts a server.\nRecording and MIDI in the standalone shell remain unimplemented.\n";

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("shr-rec: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let mut root = None;
    let mut init = false;
    let mut library = None;
    let mut open = None;
    let mut args = env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--help" | "-h") => {
                print!("{HELP}");
                return Ok(());
            }
            Some("--version" | "-V") => {
                println!("shr-rec {}", env!("CARGO_PKG_VERSION"));
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
            Some("--takes-root") => {
                let value = args.next().context("--takes-root requires a path")?;
                if value.is_empty() {
                    bail!("--takes-root requires a nonempty path");
                }
                if library.replace(PathBuf::from(value)).is_some() {
                    bail!("--takes-root provided twice");
                }
            }
            Some("--open") => {
                let value = args.next().context("--open requires a take directory")?;
                if value.is_empty() {
                    bail!("--open requires a nonempty take directory");
                }
                if open.replace(PathBuf::from(value)).is_some() {
                    bail!("--open provided twice");
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
    app.playback = shr_rec::playback_ui::PlaybackUi::new(
        root.clone(),
        library.unwrap_or_else(|| root.join("takes")),
    );
    if let Some(path) = open {
        app.playback.active = true;
        app.playback.open(&path)?;
    }
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
            app.shutdown();
            if app.session.transport() == Transport::Exited {
                break;
            }
        }
        app.tick();
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
