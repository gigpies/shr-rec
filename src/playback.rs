//! Bounded disk prefetch and a single, allocation-free callback timeline.
use crate::take::Take;
use anyhow::{Context, Result, ensure};
use rtrb::{Consumer, RingBuffer};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Seek, SeekFrom},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub const MAX_PERIOD: usize = 8192;
const PREFETCH_FRAMES: usize = 32768;
const BLOCK: usize = 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum State {
    Paused = 0,
    Playing = 1,
    Eof = 2,
    Underrun = 3,
    ReadError = 4,
    DeviceLost = 5,
    Stopped = 6,
    RateMismatch = 7,
    Xrun = 8,
    RouteLost = 9,
    InvalidPeriod = 10,
}
impl State {
    fn from(v: u32) -> Self {
        match v {
            0 => Self::Paused,
            1 => Self::Playing,
            2 => Self::Eof,
            3 => Self::Underrun,
            4 => Self::ReadError,
            5 => Self::DeviceLost,
            7 => Self::RateMismatch,
            8 => Self::Xrun,
            9 => Self::RouteLost,
            10 => Self::InvalidPeriod,
            _ => Self::Stopped,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mix {
    pub level: f32,
    pub pan: f32,
    pub mute: bool,
}
impl Default for Mix {
    fn default() -> Self {
        Self {
            level: 1.,
            pan: 0.,
            mute: false,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum Routing {
    Exact {
        outputs: Vec<String>,
    },
    Stereo {
        left: String,
        right: String,
        stems: Vec<Mix>,
    },
}
impl Routing {
    pub fn ports(&self) -> Vec<String> {
        match self {
            Self::Exact { outputs } => outputs.clone(),
            Self::Stereo { left, right, .. } => vec![left.clone(), right.clone()],
        }
    }
    pub fn validate(&self, stems: usize) -> Result<()> {
        let ports = self.ports();
        ensure!(
            !ports.is_empty() && ports.len() <= 64,
            "assign outputs explicitly"
        );
        let mut unique = std::collections::HashSet::new();
        for port in &ports {
            ensure!(
                !port.is_empty()
                    && port.len() <= 256
                    && !port.chars().any(char::is_control)
                    && unique.insert(port),
                "assign unique exact output ports"
            );
        }
        match self {
            Self::Exact { outputs } => {
                ensure!(outputs.len() == stems, "assign one exact output per stem")
            }
            Self::Stereo { stems: mix, .. } => {
                ensure!(mix.len() == stems, "mix must match take channel order");
                for m in mix {
                    ensure!(
                        m.level.is_finite()
                            && (0. ..=1.).contains(&m.level)
                            && m.pan.is_finite()
                            && (-1. ..=1.).contains(&m.pan),
                        "level 0–1, pan -1–1 required"
                    );
                }
            }
        }
        Ok(())
    }
    fn matrix(&self, count: usize) -> Vec<Vec<f32>> {
        match self {
            Self::Exact { .. } => (0..count)
                .map(|i| (0..count).map(|j| if i == j { 1. } else { 0. }).collect())
                .collect(),
            // Fixed 0.5/N gain, including muted stems, reserves 6 dB below
            // full scale even for coherent stems and floating-point rounding.
            // Linear pan never increases a stem above unity. Master stays 100%.
            Self::Stereo { stems, .. } => stems
                .iter()
                .map(|m| {
                    let gain = if m.mute {
                        0.
                    } else {
                        m.level * 0.5 / count as f32
                    };
                    vec![gain * (1. - m.pan.max(0.)), gain * (1. + m.pan.min(0.))]
                })
                .collect(),
        }
    }
}
pub struct Shared {
    state: AtomicU32,
    position: AtomicU64,
    cancel: AtomicBool,
    finished: AtomicBool,
    failed: AtomicBool,
    detail: Mutex<Option<String>>,
}
impl Shared {
    pub fn state(&self) -> State {
        let state = State::from(self.state.load(Ordering::Acquire));
        if self.failed.load(Ordering::Acquire) && matches!(state, State::Playing | State::Paused) {
            State::ReadError
        } else {
            state
        }
    }
    pub fn position(&self) -> u64 {
        self.position.load(Ordering::Acquire)
    }
    pub fn play(&self) -> Result<()> {
        self.state
            .compare_exchange(
                State::Paused as u32,
                State::Playing as u32,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map_err(|_| anyhow::anyhow!("stop/seek after EOF or fault"))?;
        Ok(())
    }
    pub fn pause(&self) {
        let _ = self.state.compare_exchange(
            State::Playing as u32,
            State::Paused as u32,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
    /// Owner-thread diagnostic only; never called by Render or JACK callbacks.
    pub fn failure_detail(&self) -> Option<String> {
        self.detail.lock().ok().and_then(|s| s.clone())
    }
    pub(crate) fn fault(&self, state: State) {
        self.state.store(state as u32, Ordering::Release);
    }
    pub fn device_lost(&self) {
        self.state
            .store(State::DeviceLost as u32, Ordering::Release);
    }
    pub fn stop(&self) {
        self.state.store(State::Stopped as u32, Ordering::Release);
        self.cancel.store(true, Ordering::Release);
    }
}
/// This object and its queue consumer are exclusively owned by the audio thread
/// while active. UI observes Shared only; destruction happens after deactivation.
pub struct Render {
    queue: Consumer<f32>,
    pub shared: Arc<Shared>,
    channels: usize,
    end: u64,
}
impl Render {
    pub fn channels(&self) -> usize {
        self.channels
    }
    /// Inspect only while owning Render exclusively, as in a null-callback host.
    /// Active device owners must use Shared snapshots instead.
    pub fn ready_frames(&self) -> usize {
        self.queue.slots() / self.channels
    }
    pub fn capacity_frames(&self) -> usize {
        PREFETCH_FRAMES
    }
    /// All output slices have the same callback length. Invalid/oversize blocks,
    /// disk failure or shortage silence the WHOLE period and stop advancement.
    pub fn process(&mut self, outputs: &mut [&mut [f32]]) {
        let frames = outputs.first().map_or(0, |o| o.len());
        for output in outputs.iter_mut() {
            output.fill(0.);
        }
        if self.shared.state() != State::Playing {
            return;
        }
        if frames == 0
            || frames > MAX_PERIOD
            || outputs.len() != self.channels
            || outputs.iter().any(|o| o.len() != frames)
        {
            self.shared
                .state
                .store(State::InvalidPeriod as u32, Ordering::Release);
            return;
        }
        if self.shared.failed.load(Ordering::Acquire) {
            self.shared
                .state
                .store(State::ReadError as u32, Ordering::Release);
            return;
        }
        let position = self.shared.position();
        let count = frames.min((self.end - position) as usize);
        if self.queue.slots() < count * self.channels {
            let _ = self.shared.state.compare_exchange(
                State::Playing as u32,
                State::Underrun as u32,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            return;
        }
        for frame in 0..count {
            for output in outputs.iter_mut() {
                output[frame] = self
                    .queue
                    .pop()
                    .expect("whole callback availability checked");
            }
        }
        self.shared
            .position
            .store(position + count as u64, Ordering::Release);
        if position + count as u64 == self.end {
            let _ = self.shared.state.compare_exchange(
                State::Playing as u32,
                State::Eof as u32,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
    }
}
pub struct Prefetch {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}
impl Drop for Prefetch {
    fn drop(&mut self) {
        self.shared.cancel.store(true, Ordering::Release);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

/// Startup/seek happens off callback. The host must quiesce the old Render before
/// replacing it. A new stream starts paused, fully primed or at the final tail.
pub fn prepare(
    mut take: Take,
    routing: &Routing,
    rate: u32,
    allow_damaged: bool,
    start: u64,
) -> Result<(Render, Prefetch)> {
    take.authorize(rate, allow_damaged)?;
    routing.validate(take.stems.len())?;
    ensure!(start < take.frames, "seek must be inside take");
    let channels = routing.ports().len();
    let matrix = routing.matrix(take.stems.len());
    let end = take.frames;
    let (mut producer, consumer) = RingBuffer::new(PREFETCH_FRAMES * channels);
    let shared = Arc::new(Shared {
        state: AtomicU32::new(0),
        position: AtomicU64::new(start),
        cancel: AtomicBool::new(false),
        finished: AtomicBool::new(false),
        failed: AtomicBool::new(false),
        detail: Mutex::new(None),
    });
    let writer_shared = shared.clone();
    let worker = thread::Builder::new()
        .name("rec-play-prefetch".into())
        .spawn(move || {
            let result = (|| -> Result<()> {
                for stem in &mut take.stems {
                    stem.file
                        .seek(SeekFrom::Start(stem.data_offset + start * 3))?;
                }
                let mut bytes = vec![0u8; BLOCK * 3];
                let mut samples = vec![0f32; BLOCK * channels];
                let mut cursor = start;
                while cursor < end && !writer_shared.cancel.load(Ordering::Acquire) {
                    let count = BLOCK.min((end - cursor) as usize);
                    if producer.slots() < count * channels {
                        thread::sleep(Duration::from_millis(1));
                        continue;
                    }
                    samples[..count * channels].fill(0.);
                    for (stem, gains) in take.stems.iter_mut().zip(&matrix) {
                        stem.file
                            .read_exact(&mut bytes[..count * 3])
                            .with_context(|| format!("read stem {} during playback", stem.label))?;
                        for f in 0..count {
                            let b = &bytes[f * 3..f * 3 + 3];
                            let sample = ((i32::from(b[0])
                                | (i32::from(b[1]) << 8)
                                | (i32::from(b[2]) << 16))
                                << 8
                                >> 8) as f32
                                / 8388608.;
                            for (channel, gain) in gains.iter().enumerate() {
                                samples[f * channels + channel] += sample * gain;
                            }
                        }
                    }
                    for &sample in &samples[..count * channels] {
                        producer.push(sample).expect("producer capacity checked");
                    }
                    cursor += count as u64;
                }
                Ok(())
            })();
            if let Err(error) = result {
                if let Ok(mut detail) = writer_shared.detail.lock() {
                    *detail = Some(format!("{error:#}"));
                }
                writer_shared.failed.store(true, Ordering::Release);
            }
            writer_shared.finished.store(true, Ordering::Release);
        })?;
    let prefetch = Prefetch {
        shared: shared.clone(),
        worker: Some(worker),
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    let target = (end - start).min((PREFETCH_FRAMES - BLOCK) as u64) as usize * channels;
    while consumer.slots() < target
        && !shared.failed.load(Ordering::Acquire)
        && !shared.finished.load(Ordering::Acquire)
    {
        ensure!(Instant::now() < deadline, "prefetch startup timed out");
        thread::sleep(Duration::from_millis(1));
    }
    ensure!(
        !shared.failed.load(Ordering::Acquire),
        "cannot read playback stems: {}",
        shared.failure_detail().unwrap_or_default()
    );
    Ok((
        Render {
            queue: consumer,
            shared,
            channels,
            end,
        },
        prefetch,
    ))
}
