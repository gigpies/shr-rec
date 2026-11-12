mod support;
use shr_rec::{
    playback::{self, Mix, Routing, State},
    recorder::{Config as CaptureConfig, Recorder, recover},
    take::{Take, discover},
};
use std::{
    fs,
    io::{Seek, SeekFrom, Write},
    os::unix::fs::symlink,
};
use support::*;

#[test]
fn shr_order_metadata_read_only_pause_tail_eof_and_seek() {
    let root = Scratch::new();
    let path = shr(&root.0, 2071, 3);
    let before = fs::read(path.join("session.json")).unwrap();
    let take = Take::open(&path).unwrap();
    assert_eq!(take.stems[2].id, "id-2");
    assert!(take.warning.is_none());
    let (mut render, worker) = playback::prepare(take, &exact(3), 48000, false, 0).unwrap();
    let shared = render.shared.clone();
    let mut channels = vec![vec![99.; 256]; 3];
    {
        let mut out: Vec<_> = channels.iter_mut().map(Vec::as_mut_slice).collect();
        render.process(&mut out);
    }
    assert_eq!(shared.position(), 0);
    assert!(channels.iter().flatten().all(|s| *s == 0.));
    shared.play().unwrap();
    let mut cursor = 0;
    while shared.state() == State::Playing {
        {
            let mut out: Vec<_> = channels.iter_mut().map(Vec::as_mut_slice).collect();
            render.process(&mut out);
        }
        for (stem, data) in channels.iter().enumerate() {
            for (i, &s) in data.iter().enumerate() {
                assert_eq!(
                    s,
                    if cursor + i < 2071 {
                        sample(stem, (cursor + i) as u64) as f32 / 8388608.
                    } else {
                        0.
                    }
                );
            }
        }
        cursor += 256;
        if cursor == 256 {
            shared.pause();
            {
                let mut out: Vec<_> = channels.iter_mut().map(Vec::as_mut_slice).collect();
                render.process(&mut out);
            }
            assert_eq!(shared.position(), 256);
            shared.play().unwrap();
        }
    }
    assert_eq!(shared.state(), State::Eof);
    assert_eq!(shared.position(), 2071);
    assert!(shared.play().is_err());
    drop(worker);
    drop(render);
    let (mut render, _worker) =
        playback::prepare(Take::open(&path).unwrap(), &exact(3), 48000, false, 2000).unwrap();
    render.shared.play().unwrap();
    let mut a = [0.; 128];
    let mut b = [0.; 128];
    let mut c = [0.; 128];
    render.process(&mut [&mut a, &mut b, &mut c]);
    assert_eq!(a[0], sample(0, 2000) as f32 / 8388608.);
    assert_eq!(c[70], sample(2, 2070) as f32 / 8388608.);
    assert_eq!(b[71], 0.);
    assert_eq!(render.shared.state(), State::Eof);
    assert_eq!(before, fs::read(path.join("session.json")).unwrap());
    assert_eq!(fs::read_dir(&path).unwrap().count(), 4);
}

#[test]
fn native_capture_finalize_reopen_and_recovered_selection() {
    let root = Scratch::new();
    let path = root.0.join("native");
    let mut recorder = Recorder::create(
        &path,
        CaptureConfig {
            sample_rate: 48000,
            channels: 2,
            max_block: 32,
            queue_blocks: 8,
            epoch: 29,
        },
    )
    .unwrap();
    // Live ownership prevents opening even if a manifest is present.
    assert!(Take::open(&path).is_err());
    let samples: Vec<_> = (0..31)
        .flat_map(|f| {
            [
                sample(0, f) as f64 / 8388608.,
                sample(1, f) as f64 / 8388608.,
            ]
        })
        .collect();
    assert_eq!(recorder.push(100, &samples, 31), 0);
    assert_eq!(recorder.finish().unwrap().status, "complete");
    let take = Take::open(&path).unwrap();
    assert!(take.warning.is_none());
    assert_eq!(take.frames, 31);
    assert_eq!(take.stems.len(), 2);
    let (mut render, _worker) = playback::prepare(take, &exact(2), 48000, false, 0).unwrap();
    render.shared.play().unwrap();
    let mut a = [0.; 32];
    let mut b = [0.; 32];
    render.process(&mut [&mut a, &mut b]);
    assert_eq!(a[30], 31. / 8388608.);
    assert_eq!(b[30], -1031. / 8388608.);
    assert_eq!(b[31], 0.);
    let recovered = root.0.join("copy");
    recover(&path, &recovered).unwrap();
    let take = Take::open(&recovered).unwrap();
    assert!(take.warning.is_some());
    assert!(take.authorize(48000, false).is_err());
    assert!(take.authorize(48000, true).is_ok());
    assert!(take.authorize(44100, true).is_err());
}

#[test]
fn rejects_unsafe_newer_malformed_misaligned_or_ambiguous_takes() {
    let root = Scratch::new();
    let path = shr(&root.0, 9, 2);
    let original = fs::read(path.join("session.json")).unwrap();
    for change in [0, 1, 2, 3, 4, 5, 6, 7] {
        edit(&path, |m| match change {
            0 => m["format_version"] = 2.into(),
            1 => m["tracks"][0]["wav_file"] = "../escape.wav".into(),
            2 => m["tracks"][1]["id"] = "id-0".into(),
            3 => m["tracks"][1]["wav_file"] = "stem-0.wav".into(),
            4 => m["tracks"][0]["frames"] = 8.into(),
            5 => m["tracks"][0]["finalized"] = false.into(),
            6 => m["sample_rate"] = 44100.into(),
            _ => m["total_frames"] = 0.into(),
        });
        assert!(Take::open(&path).is_err(), "change {change}");
        fs::write(path.join("session.json"), &original).unwrap();
    }
    let stem = path.join("stem-1.wav");
    let bytes = fs::read(&stem).unwrap();
    fs::remove_file(&stem).unwrap();
    symlink(path.join("stem-0.wav"), &stem).unwrap();
    assert!(Take::open(&path).is_err());
    fs::remove_file(&stem).unwrap();
    fs::hard_link(path.join("stem-0.wav"), &stem).unwrap();
    assert!(Take::open(&path).is_err());
    fs::remove_file(&stem).unwrap();
    fs::write(&stem, &bytes[..bytes.len() - 1]).unwrap();
    assert!(Take::open(&path).is_err());
    fs::write(&stem, bytes).unwrap();
    symlink(&path, root.0.join("alias")).unwrap();
    assert!(Take::open(&root.0.join("alias")).is_err());
    assert_eq!(discover(&root.0).unwrap().len(), 1);
    let mut f = fs::OpenOptions::new()
        .write(true)
        .open(path.join("stem-0.wav"))
        .unwrap();
    f.seek(SeekFrom::Start(34)).unwrap();
    f.write_all(&16u16.to_le_bytes()).unwrap();
    assert!(Take::open(&path).is_err());
}

#[test]
fn damaged_flags_are_visible_and_rate_mismatch_never_resamples() {
    let root = Scratch::new();
    let path = shr(&root.0, 12, 1);
    edit(&path, |m| m["xruns"] = 1.into());
    let take = Take::open(&path).unwrap();
    assert!(take.warning.is_some());
    assert!(take.authorize(48000, false).is_err());
    assert!(
        take.authorize(44100, true)
            .unwrap_err()
            .to_string()
            .contains("resampling unsupported")
    );
    assert!(take.authorize(48000, true).is_ok());
    edit(&path, |m| {
        m["completeness"] = "recovered".into();
        m["finalization"] = "finalized-with-error".into();
    });
    assert!(Take::open(&path).unwrap().warning.is_some());
}

#[test]
fn stereo_level_pan_mute_and_fixed_headroom_are_explicit() {
    let root = Scratch::new();
    let path = shr(&root.0, 8, 3);
    let route = Routing::Stereo {
        left: "sink:left".into(),
        right: "sink:right".into(),
        stems: vec![
            Mix {
                level: 1.,
                pan: -1.,
                mute: false,
            },
            Mix {
                level: 0.5,
                pan: 1.,
                mute: false,
            },
            Mix {
                level: 1.,
                pan: 0.,
                mute: true,
            },
        ],
    };
    let (mut r, _w) =
        playback::prepare(Take::open(&path).unwrap(), &route, 48000, false, 0).unwrap();
    r.shared.play().unwrap();
    let mut a = [0.; 8];
    let mut b = [0.; 8];
    r.process(&mut [&mut a, &mut b]);
    for f in 0..8 {
        assert!((a[f] - sample(0, f as u64) as f32 / 8388608. / 6.).abs() < 1e-10);
        assert!((b[f] - sample(1, f as u64) as f32 / 8388608. / 12.).abs() < 1e-10);
    }
    assert!(
        Routing::Exact {
            outputs: vec!["sink:1".into(); 3]
        }
        .validate(3)
        .is_err()
    );
    assert!(
        Routing::Stereo {
            left: "x".into(),
            right: "y".into(),
            stems: vec![
                Mix {
                    level: f32::NAN,
                    pan: 0.,
                    mute: false
                };
                3
            ]
        }
        .validate(3)
        .is_err()
    );
}

#[test]
fn startup_failure_then_reopen_leaves_no_source_mutation() {
    let root = Scratch::new();
    let path = shr(&root.0, 8192, 2);
    assert!(playback::prepare(Take::open(&path).unwrap(), &exact(2), 44100, false, 0).is_err());
    let take = Take::open(&path).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(path.join("stem-1.wav"))
        .unwrap()
        .set_len(44)
        .unwrap();
    assert!(playback::prepare(take, &exact(2), 48000, false, 0).is_err());
    wav(&path.join("stem-1.wav"), 48000, 8192, 1);
    let (r, w) = playback::prepare(Take::open(&path).unwrap(), &exact(2), 48000, false, 0).unwrap();
    drop(w);
    drop(r);
    assert!(Take::open(&path).is_ok());
}

#[test]
fn shr_legacy_odd_data_tail_without_riff_padding_is_supported_read_only() {
    let root = Scratch::new();
    let path = shr(&root.0, 9, 1);
    let stem = path.join("stem-0.wav");
    let mut bytes = fs::read(&stem).unwrap();
    bytes.pop();
    let len = (bytes.len() - 8) as u32;
    bytes[4..8].copy_from_slice(&len.to_le_bytes());
    fs::write(&stem, &bytes).unwrap();
    assert!(Take::open(&path).is_ok());
    assert_eq!(fs::read(&stem).unwrap(), bytes);
}

#[test]
fn stopped_prefetch_causes_whole_period_underrun_without_frame_drift() {
    let root = Scratch::new();
    let path = shr(&root.0, 100000, 2);
    let (mut render, worker) =
        playback::prepare(Take::open(&path).unwrap(), &exact(2), 48000, false, 0).unwrap();
    drop(worker);
    render.shared.play().unwrap();
    let mut a = [99.; 8192];
    let mut b = [99.; 8192];
    let mut cursor = 0;
    for _ in 0..6 {
        render.process(&mut [&mut a, &mut b]);
        if render.shared.state() == State::Underrun {
            assert_eq!(render.shared.position(), cursor);
            assert!(a.iter().chain(&b).all(|v| *v == 0.));
            break;
        }
        assert_eq!(a[0], sample(0, cursor) as f32 / 8388608.);
        assert_eq!(b[8191], sample(1, cursor + 8191) as f32 / 8388608.);
        cursor += 8192;
    }
    assert_eq!(render.shared.state(), State::Underrun);
    assert!(render.shared.play().is_err());
    // A retry/seek gets fresh queues for every stem, never the abandoned tail.
    let (mut next, _worker) =
        playback::prepare(Take::open(&path).unwrap(), &exact(2), 48000, false, 12000).unwrap();
    next.shared.play().unwrap();
    next.process(&mut [&mut a, &mut b]);
    assert_eq!(a[0], sample(0, 12000) as f32 / 8388608.);
    assert_eq!(b[0], sample(1, 12000) as f32 / 8388608.);
}

#[test]
fn midstream_disk_failure_stops_all_stems_and_keeps_precise_owner_diagnostic() {
    use std::time::{Duration, Instant};
    let root = Scratch::new();
    let path = shr(&root.0, 100000, 2);
    let (mut render, _worker) =
        playback::prepare(Take::open(&path).unwrap(), &exact(2), 48000, false, 0).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(path.join("stem-1.wav"))
        .unwrap()
        .set_len(44)
        .unwrap();
    render.shared.play().unwrap();
    let mut a = [0.; 8192];
    let mut b = [0.; 8192];
    render.process(&mut [&mut a, &mut b]);
    let deadline = Instant::now() + Duration::from_secs(2);
    while render.shared.state() != State::ReadError && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(render.shared.state(), State::ReadError);
    let position = render.shared.position();
    a.fill(99.);
    b.fill(99.);
    render.process(&mut [&mut a, &mut b]);
    assert_eq!(render.shared.position(), position);
    assert!(a.iter().chain(&b).all(|v| *v == 0.));
    assert!(render.shared.failure_detail().unwrap().contains("Stem 1"));
}

#[test]
fn bounded_stream_reads_beyond_prefetch_and_18_and_64_channels_keep_order() {
    use std::time::{Duration, Instant};
    for count in [18, 64] {
        let root = Scratch::new();
        let path = shr(&root.0, 1100, count);
        let (mut render, _worker) =
            playback::prepare(Take::open(&path).unwrap(), &exact(count), 48000, false, 0).unwrap();
        render.shared.play().unwrap();
        let mut channels = vec![vec![0.; 1200]; count];
        let mut out: Vec<_> = channels.iter_mut().map(Vec::as_mut_slice).collect();
        render.process(&mut out);
        assert_eq!(render.shared.position(), 1100);
        for (stem, data) in channels.iter().enumerate() {
            for (f, s) in data.iter().enumerate().take(1100) {
                assert_eq!(*s, sample(stem, f as u64) as f32 / 8388608.);
            }
            assert!(data[1100..].iter().all(|v| *v == 0.));
        }
    }
    let root = Scratch::new();
    let path = shr(&root.0, 100000, 2);
    let (mut render, _worker) =
        playback::prepare(Take::open(&path).unwrap(), &exact(2), 48000, false, 0).unwrap();
    assert_eq!(render.capacity_frames(), 32768);
    assert!(render.ready_frames() <= 32768);
    // This frame is beyond both the ring and a worker's one-block scratch.
    // Seeing its subsequent value proves streaming rather than whole-file load.
    overwrite_sample(&path.join("stem-0.wav"), 50000, 777777);
    render.shared.play().unwrap();
    let mut a = [0.; 1024];
    let mut b = [0.; 1024];
    let mut cursor = 0u64;
    while render.shared.state() == State::Playing {
        let needed = 1024.min((100000 - cursor) as usize);
        let deadline = Instant::now() + Duration::from_secs(2);
        while render.ready_frames() < needed && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            render.ready_frames() >= needed,
            "prefetch did not replenish"
        );
        render.process(&mut [&mut a, &mut b]);
        for i in 0..needed {
            assert_eq!(
                a[i],
                if cursor + i as u64 == 50000 {
                    777777.
                } else {
                    sample(0, cursor + i as u64) as f32
                } / 8388608.
            );
            assert_eq!(b[i], sample(1, cursor + i as u64) as f32 / 8388608.);
        }
        cursor += needed as u64;
    }
    assert_eq!(render.shared.position(), 100000);
    assert_eq!(render.shared.state(), State::Eof);
}

#[test]
fn partial_duplicate_and_finalization_failure_marker_never_become_normal_complete() {
    let root = Scratch::new();
    let path = shr(&root.0, 9, 2);
    fs::copy(path.join("stem-0.wav"), path.join("stem-0.wav.part")).unwrap();
    assert!(
        Take::open(&path)
            .err()
            .unwrap()
            .to_string()
            .contains("ambiguous")
    );
    fs::remove_file(path.join("stem-0.wav.part")).unwrap();
    assert!(Take::open(&path).is_ok());
    let native = root.0.join("native");
    let mut r = Recorder::create(
        &native,
        CaptureConfig {
            sample_rate: 48000,
            channels: 1,
            max_block: 8,
            queue_blocks: 2,
            epoch: 1,
        },
    )
    .unwrap();
    assert_eq!(r.push(0, &[0.25; 8], 8), 0);
    r.finish().unwrap();
    let report = fs::read(native.join("result.json")).unwrap();
    fs::write(native.join("failure.json"), report).unwrap();
    let take = Take::open(&native).unwrap();
    assert!(take.warning.unwrap().contains("failure marker"));
}

#[test]
fn duplicate_json_keys_and_malformed_stereo_metadata_are_rejected() {
    let root = Scratch::new();
    let path = shr(&root.0, 16, 2);
    let original = fs::read_to_string(path.join("session.json")).unwrap();
    fs::write(
        path.join("session.json"),
        format!("{{\"format_version\":2,{}", &original[1..]),
    )
    .unwrap();
    assert!(
        Take::open(&path)
            .err()
            .unwrap()
            .to_string()
            .contains("parse")
    );
    fs::write(path.join("session.json"), &original).unwrap();
    edit(&path, |m| {
        m["tracks"][0]["role"] = "stereo-left".into();
        m["tracks"][0]["group"] = "pair".into();
    });
    assert!(Take::open(&path).is_err());
    edit(&path, |m| {
        m["tracks"][1]["role"] = "stereo-right".into();
        m["tracks"][1]["group"] = "pair".into();
    });
    assert!(Take::open(&path).is_ok());
    edit(&path, |m| m["duration_seconds"] = 5.into());
    assert!(Take::open(&path).is_err());
}

#[test]
fn exact_playback_preserves_pcm24_extremes_at_unity() {
    let root = Scratch::new();
    let path = shr(&root.0, 3, 1);
    for (frame, value) in [-8388608, 8388607, 1].into_iter().enumerate() {
        overwrite_sample(&path.join("stem-0.wav"), frame as u64, value);
    }
    let (mut render, _worker) =
        playback::prepare(Take::open(&path).unwrap(), &exact(1), 48000, false, 0).unwrap();
    render.shared.play().unwrap();
    let mut out = [99.; 4];
    render.process(&mut [&mut out]);
    assert_eq!(out, [-1., 8388607. / 8388608., 1. / 8388608., 0.]);
}
