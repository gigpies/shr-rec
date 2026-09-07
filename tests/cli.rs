use rec::config::Config;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};
static SERIAL: AtomicUsize = AtomicUsize::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rec-test-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rec"))
        .args(args)
        .env_remove("REC_DATA_ROOT")
        .env_remove("XDG_DATA_HOME")
        .env_remove("HOME")
        .output()
        .unwrap()
}
#[test]
fn help_and_version_work_without_home_or_terminal() {
    let output = run(&["--help"]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("--data-root"));
    let output = run(&["--version"]);
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "rec 0.1.0\n");
    assert!(!run(&["--data-root"]).status.success());
    assert!(!run(&["--unknown"]).status.success());
}
#[test]
fn explicit_setup_is_private_create_only_and_reading_is_inert() {
    let scratch = Scratch::new();
    let root = scratch.0.join("private");
    assert_eq!(Config::load(&root).unwrap().tracks.len(), 18);
    assert!(!root.exists());
    let output = run(&["--data-root", root.to_str().unwrap(), "--init-config"]);
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(
        fs::metadata(&root).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let path = root.join("config.json");
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let bytes = fs::read(&path).unwrap();
    assert!(Config::load(&root).is_ok());
    assert!(
        !run(&["--data-root", root.to_str().unwrap(), "--init-config"])
            .status
            .success()
    );
    assert_eq!(fs::read(&path).unwrap(), bytes);
    let output = run(&["--data-root", root.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("interactive terminal required"));
    assert_eq!(fs::read(&path).unwrap(), bytes);
}
#[test]
fn config_refuses_symlink_directory_fifo_oversize_and_malformed() {
    let scratch = Scratch::new();
    let path = scratch.0.join("config.json");
    let target = scratch.0.join("target.json");
    fs::write(&target, serde_json::to_vec(&Config::default()).unwrap()).unwrap();
    symlink(&target, &path).unwrap();
    assert!(Config::load(&scratch.0).is_err());
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(Config::load(&scratch.0).is_err());
    fs::remove_dir(&path).unwrap();
    assert!(
        Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    assert!(Config::load(&scratch.0).is_err());
    fs::remove_file(&path).unwrap();
    let file = fs::File::create(&path).unwrap();
    file.set_len(128 * 1024 + 1).unwrap();
    assert!(Config::load(&scratch.0).is_err());
    fs::write(&path, b"{bad json}").unwrap();
    assert!(Config::load(&scratch.0).is_err());
}

#[test]
fn pseudo_terminal_exit_touch_signal_and_raw_mode_restoration() {
    // This exercises the production binary with an isolated PTY, never audio.
    let output = Command::new("python3")
        .arg("tests/terminal.py")
        .arg(env!("CARGO_BIN_EXE_rec"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
