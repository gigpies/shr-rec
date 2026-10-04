use crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use shr_rec::{
    config::{Config, Resolution},
    session::{Effect, RetainedTake, Session, Transport},
    ui::{Action, App},
};

fn retained(complete: bool) -> RetainedTake {
    RetainedTake {
        path: "test.take".into(),
        complete,
        detail: if complete {
            "Complete"
        } else {
            "Disk failure; review retained files"
        }
        .into(),
    }
}

#[test]
fn exit_cancel_preserves_capture_and_confirmation_waits_for_worker() {
    let mut session = Session::default();
    assert_eq!(session.record(), Ok(Effect::PrepareCapture(1)));
    session
        .capture_ready(session.operation().unwrap_or(1))
        .unwrap();
    assert_eq!(session.request_exit(), Effect::None);
    assert!(session.exit_confirmation());
    session.cancel_exit();
    assert_eq!(session.transport(), Transport::Recording);
    session.request_exit();
    assert_eq!(session.confirm_exit(), Effect::QuiesceAndFinalize(1));
    assert_eq!(session.transport(), Transport::Finalizing);
    assert_eq!(session.stop(), Effect::None);
    assert_eq!(
        session.capture_finished(session.operation().unwrap_or(1), retained(true)),
        Ok(Effect::ReturnToCaller)
    );
    assert_eq!(session.transport(), Transport::Exited);
}

#[test]
fn damaged_finalization_stays_visible_before_deliberate_exit() {
    let mut session = Session::default();
    session.record().unwrap();
    session.request_exit();
    session.confirm_exit();
    assert!(
        session
            .capture_ready(session.operation().unwrap_or(1))
            .is_err()
    );
    assert_eq!(
        session.capture_finished(session.operation().unwrap_or(1), retained(false)),
        Ok(Effect::None)
    );
    assert_eq!(session.transport(), Transport::Stopped);
    assert!(!session.retained().unwrap().complete);
    assert_eq!(session.request_exit(), Effect::ReturnToCaller);
}

#[test]
fn startup_failure_can_retry_but_late_ready_cannot_rearm() {
    let mut session = Session::default();
    session.record().unwrap();
    session
        .capture_finished(session.operation().unwrap_or(1), retained(false))
        .unwrap();
    assert!(
        session
            .capture_ready(session.operation().unwrap_or(1))
            .is_err()
    );
    assert_eq!(session.record(), Ok(Effect::PrepareCapture(2)));
    assert!(session.capture_ready(1).is_err());
    assert!(session.capture_finished(1, retained(true)).is_err());
    session.capture_ready(2).unwrap();
    assert_eq!(session.transport(), Transport::Recording);
}

#[test]
fn capture_and_playback_are_exclusive_through_shutdown() {
    let mut session = Session::default();
    session.record().unwrap();
    assert!(session.play().is_err());
    session
        .capture_ready(session.operation().unwrap_or(1))
        .unwrap();
    assert!(session.play().is_err());
    session.stop();
    assert!(session.play().is_err());
    session
        .capture_finished(session.operation().unwrap_or(1), retained(true))
        .unwrap();
    session.play().unwrap();
    assert!(session.record().is_err());
    session.pause().unwrap();
    assert!(session.record().is_err());
    assert_eq!(session.play(), Ok(Effect::ResumePlayback));
    assert_eq!(session.request_exit(), Effect::QuiescePlayback);
    assert_eq!(session.transport(), Transport::StoppingPlayback);
    assert!(session.record().is_err());
    assert_eq!(session.playback_stopped(), Ok(Effect::ReturnToCaller));
}

#[test]
fn exact_route_refresh_never_rewrites_preference() {
    let mut config = Config::default();
    config.tracks[0].armed = true;
    config.tracks[0].preferred_source = Some("fixture:channel-1".into());
    assert!(
        config
            .capture_ready(&["fixture:channel-10".into()])
            .is_err()
    );
    assert_eq!(config.tracks[0].resolve(&[]), Resolution::Missing);
    assert_eq!(
        config.tracks[0].resolve(&["fixture:channel-1".into(), "fixture:channel-1".into()]),
        Resolution::Ambiguous
    );
    assert!(config.capture_ready(&["fixture:channel-1".into()]).is_ok());
    assert_eq!(
        config.tracks[0].preferred_source.as_deref(),
        Some("fixture:channel-1")
    );
    config.tracks[1].armed = true;
    assert!(config.capture_ready(&["fixture:channel-1".into()]).is_err());
}

#[test]
fn config_rejects_unsafe_identity_unknown_version_and_over_limit() {
    let mut config = Config::default();
    config.tracks[1].id = config.tracks[0].id.clone();
    assert!(config.validate().is_err());
    config.tracks[1].id = "../escape".into();
    assert!(config.validate().is_err());
    config = Config::default();
    config.format_version = 2;
    assert!(config.validate().is_err());
    config = Config::default();
    config.tracks = (0..65)
        .map(|i| {
            let mut track = config.tracks[0].clone();
            track.id = format!("id-{i}");
            track
        })
        .collect();
    assert!(config.validate().is_err());
}

fn tap(x: u16, y: u16) -> Event {
    Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    })
}
fn row(terminal: &Terminal<TestBackend>, y: u16) -> String {
    (0..40)
        .map(|x| terminal.backend().buffer().get(x, y).symbol.as_str())
        .collect()
}

#[test]
fn native_touch_navigation_arm_status_and_exit() {
    let mut app = App::new(Config::default());
    let size = Rect::new(0, 0, 40, 13);
    let mut terminal = Terminal::new(TestBackend::new(40, 13)).unwrap();
    for _ in 0..17 {
        app.dispatch(app.event(tap(15, 10), size).unwrap());
    }
    assert_eq!(app.selected, 17);
    app.dispatch(app.event(tap(25, 10), size).unwrap());
    assert!(app.config.tracks[17].armed);
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert!(row(&terminal, 6).contains("Input 18"));
    assert!(row(&terminal, 10).contains("RECORD"));
    assert!(row(&terminal, 11).contains("EXIT"));
    assert!(row(&terminal, 12).starts_with("■ Arm changes"));
    app.dispatch(Action::Record);
    assert_eq!(app.session.transport(), Transport::Stopped);
    app.dispatch(app.event(tap(35, 11), size).unwrap());
    assert_eq!(app.session.transport(), Transport::Exited);
}

#[test]
fn exit_overlay_has_touch_confirm_cancel_and_preserves_status_row() {
    let mut app = App::new(Config::default());
    app.session.record().unwrap();
    app.session
        .capture_ready(app.session.operation().unwrap())
        .unwrap();
    app.dispatch(Action::Exit);
    let size = Rect::new(0, 0, 40, 13);
    let mut terminal = Terminal::new(TestBackend::new(40, 13)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert!(row(&terminal, 10).contains("STOP AND FINALIZE"));
    assert!(row(&terminal, 10).contains("CANCEL"));
    assert!(row(&terminal, 12).starts_with("● "));
    app.dispatch(app.event(tap(25, 11), size).unwrap());
    assert_eq!(app.session.transport(), Transport::Recording);
    app.dispatch(Action::Exit);
    app.dispatch(app.event(tap(5, 11), size).unwrap());
    assert_eq!(app.session.transport(), Transport::Finalizing);
}

#[test]
fn small_terminal_keeps_visible_touch_exit_without_panicking() {
    for (width, height) in [(20, 6), (1, 1), (40, 12)] {
        let mut app = App::new(Config::default());
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let action = app
            .event(tap(0, height - 1), Rect::new(0, 0, width, height))
            .unwrap();
        app.dispatch(action);
        assert_eq!(app.session.transport(), Transport::Exited);
    }
}

#[test]
fn preparing_stop_retires_ready_and_finishes_exactly_once() {
    let mut session = Session::default();
    assert_eq!(session.record(), Ok(Effect::PrepareCapture(1)));
    assert!(session.record().is_err());
    assert_eq!(session.stop(), Effect::QuiesceAndFinalize(1));
    assert_eq!(session.stop(), Effect::None);
    assert!(session.capture_ready(1).is_err());
    assert!(session.record().is_err());
    session.capture_finished(1, retained(false)).unwrap();
    assert!(session.capture_finished(1, retained(true)).is_err());
    assert_eq!(session.record(), Ok(Effect::PrepareCapture(2)));
    assert!(session.capture_ready(1).is_err());
    assert!(session.capture_finished(1, retained(false)).is_err());
    assert_eq!(session.transport(), Transport::Starting);
    session.capture_ready(2).unwrap();
}

#[test]
fn e06_corpus_runs_real_session_owner() {
    let corpus: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/crec/v1/e06.json")).unwrap();
    let operation = corpus["operation_id"].as_u64().unwrap();
    let mut session = Session::default();
    for old in 1..operation {
        assert_eq!(session.record(), Ok(Effect::PrepareCapture(old)));
        session.capture_finished(old, retained(false)).unwrap();
    }
    for vector in corpus["events"].as_array().unwrap() {
        let event = vector["event"].as_str().unwrap();
        let effect = match event {
            "start" | "duplicate_start" => session
                .record()
                .map(|e| format!("{e:?}"))
                .unwrap_or("rejected".into()),
            "old_ready" => session
                .capture_ready(operation - 1)
                .map(|_| "none".into())
                .unwrap_or("rejected".into()),
            "ready" | "late_ready" => session
                .capture_ready(operation)
                .map(|_| "none".into())
                .unwrap_or("rejected".into()),
            "old_finished" => session
                .capture_finished(operation - 1, retained(true))
                .map(|e| format!("{e:?}"))
                .unwrap_or("rejected".into()),
            "finished" | "duplicate_finished" => session
                .capture_finished(operation, retained(true))
                .map(|e| format!("{e:?}"))
                .unwrap_or("rejected".into()),
            "stop" | "duplicate_stop" => format!("{:?}", session.stop()),
            _ => panic!("unknown corpus event"),
        };
        assert_eq!(effect, vector["effect"].as_str().unwrap(), "{event}");
        assert_eq!(
            format!("{:?}", session.transport()),
            vector["state"].as_str().unwrap(),
            "{event}"
        );
    }
}

#[test]
fn stop_during_real_writer_preparation_cleans_up_before_retry() {
    use shr_rec::recorder::{Config as RecorderConfig, Recorder};
    let path = std::env::temp_dir().join(format!("shr-rec-preparing-{}", std::process::id()));
    let writer_path = path.clone();
    let mut session = Session::default();
    assert_eq!(session.record(), Ok(Effect::PrepareCapture(1)));
    let preparing = std::thread::spawn(move || {
        Recorder::create(
            &writer_path,
            RecorderConfig {
                sample_rate: 48000,
                channels: 2,
                max_block: 48,
                queue_blocks: 8,
                epoch: 9,
            },
        )
        .unwrap()
    });
    assert_eq!(session.stop(), Effect::QuiesceAndFinalize(1));
    assert_eq!(session.stop(), Effect::None);
    assert!(session.record().is_err());
    let recorder = preparing.join().unwrap();
    let observer = recorder.observer();
    assert!(session.capture_ready(1).is_err());
    // Producer was never activated. One consuming finish drains/joins the actual
    // prepared writer; only its completion acknowledges cleanup to Session.
    let report = recorder.finish().unwrap();
    assert_eq!(report.status, "incomplete");
    assert_eq!(observer.snapshot().outcome, 2);
    session
        .capture_finished(
            1,
            RetainedTake {
                path: path.clone(),
                complete: false,
                detail: "Stopped during preparation".into(),
            },
        )
        .unwrap();
    assert!(session.capture_finished(1, retained(true)).is_err());
    assert_eq!(session.record(), Ok(Effect::PrepareCapture(2)));
    assert!(session.capture_ready(1).is_err());
    std::fs::remove_dir_all(path).unwrap();
}
