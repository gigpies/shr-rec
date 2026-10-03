use shr_rec::recorder::{Config, Recorder, recover};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn killed_writer_preserves_common_frames_for_copy_recovery() {
    if let Some(path) = std::env::var_os("SHR_REC_CRASH_FIXTURE") {
        let mut recorder = Recorder::create(
            std::path::Path::new(&path),
            Config {
                sample_rate: 48_000,
                channels: 2,
                max_block: 192,
                queue_blocks: 8,
                epoch: 991,
            },
        )
        .unwrap();
        let samples: Vec<f64> = (0..192)
            .flat_map(|frame| {
                [
                    f64::from(frame) / 8_388_608.0,
                    -f64::from(frame + 1) / 8_388_608.0,
                ]
            })
            .collect();
        assert_eq!(recorder.push(5000, &samples, 192), 0);
        loop {
            std::thread::park_timeout(Duration::from_secs(1));
        }
    }
    let root = std::env::temp_dir().join(format!("shr-rec-crash-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let source = root.join("source");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "killed_writer_preserves_common_frames_for_copy_recovery",
        ])
        .env("SHR_REC_CRASH_FIXTURE", &source)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut ready = false;
    while Instant::now() < deadline {
        if std::fs::read(source.join("timeline.jsonl")).is_ok_and(|bytes| bytes.ends_with(b"\n")) {
            ready = true;
            break;
        }
        if child.try_wait().unwrap().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let _ = child.kill();
    child.wait().unwrap();
    assert!(ready, "child did not write its test period");
    let before = std::fs::read(source.join("channel-001.wav.part")).unwrap();
    assert!(!source.join("result.json").exists());
    let recovered = root.join("recovered");
    let report = recover(&source, &recovered).unwrap();
    assert_eq!(
        (report.status.as_str(), report.frames, report.epoch),
        ("recovered-incomplete", 192, 991)
    );
    assert_eq!(
        std::fs::read(source.join("channel-001.wav.part")).unwrap(),
        before
    );
    let payload = std::fs::read(recovered.join("channel-001.wav")).unwrap();
    assert_eq!(&payload[44..], &before[44..]);
    std::fs::remove_dir_all(root).unwrap();
}
