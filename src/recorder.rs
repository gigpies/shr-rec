//! Independent bounded raw recorder. The host owns the source clock and device.
//! Only `Recorder::push` and `Recorder::mark_fault` belong on a callback thread.
use anyhow::{Context, Result, ensure};
use rtrb::{Consumer, Producer, RingBuffer};
use serde::{Deserialize, Serialize};
use std::ffi::{CStr, CString, c_char};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub const MAX_WAV_FRAMES: u64 = (u32::MAX as u64 - 37) / 3;
const MAX_QUEUE_SAMPLES: usize = 8 * 1024 * 1024;
const MIN_FREE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub sample_rate: u32,
    pub channels: u32,
    pub max_block: u32,
    pub queue_blocks: u32,
    pub epoch: u64,
}

impl Config {
    fn validate(&self) -> Result<usize> {
        ensure!((8_000..=384_000).contains(&self.sample_rate), "sample rate");
        ensure!((1..=64).contains(&self.channels), "channel count");
        ensure!((1..=8192).contains(&self.max_block), "block size");
        ensure!((2..=4096).contains(&self.queue_blocks), "queue size");
        let samples = self.channels as usize * self.max_block as usize;
        ensure!(
            samples * self.queue_blocks as usize <= MAX_QUEUE_SAMPLES,
            "queue memory limit"
        );
        Ok(samples)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: String,
    version: u32,
    status: String,
    encoding: String,
    config: Config,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Block {
    source_frame: u64,
    frames: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Timeline {
    source_frame: u64,
    frames: u32,
    file_frame: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub status: String,
    pub frames: u64,
    pub epoch: u64,
    pub source_valid_bits: Option<u32>,
    pub dropped_frames: u64,
    pub overflow_blocks: u64,
    pub source_gap_frames: u64,
    pub invalid_blocks: u64,
    pub external_fault: u32,
    pub clipped_samples: u64,
    pub error: Option<String>,
}

#[derive(Default)]
struct Shared {
    stopping: AtomicBool,
    accepted: AtomicU64,
    written: AtomicU64,
    clipped: AtomicU64,
    outcome: AtomicU32,
    writer_failed: AtomicBool,
    dropped: AtomicU64,
    overflows: AtomicU64,
    gaps: AtomicU64,
    invalid: AtomicU64,
    external: AtomicU32,
    source_bits: AtomicU32,
}

/// Independent atomic status allocation. Create/clone/drop off callback only.
/// It may outlive the recorder and never borrows its producer queues.
#[derive(Clone)]
pub struct Observer {
    shared: Arc<Shared>,
    epoch: u64,
}

/// Version 2 fixed-size progress. Active counters are individually monotonic,
/// not a transactional snapshot. Terminal outcome publishes final counters.
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct Progress {
    pub version: u32,
    pub state: u32,
    pub outcome: u32,
    pub writer_fault: u32,
    pub host_fault: u32,
    pub reserved: u32,
    pub epoch: u64,
    pub accepted_frames: u64,
    pub written_frames: u64,
    pub durable_frames: u64,
    pub dropped_frames: u64,
    pub overflow_blocks: u64,
    pub gap_frames: u64,
    pub invalid_blocks: u64,
    pub clipped_samples: u64,
}

impl Observer {
    /// Bounded atomics only. Callers should poll at most 10 Hz off callback.
    pub fn snapshot(&self) -> Progress {
        let shared = &self.shared;
        let outcome = shared.outcome.load(Ordering::Acquire);
        let written = shared.written.load(Ordering::Acquire);
        Progress {
            version: 2,
            state: if outcome != 0 {
                3
            } else if shared.stopping.load(Ordering::Acquire) {
                2
            } else {
                1
            },
            outcome,
            writer_fault: u32::from(shared.writer_failed.load(Ordering::Acquire)),
            host_fault: shared.external.load(Ordering::Relaxed),
            reserved: 0,
            epoch: self.epoch,
            accepted_frames: shared.accepted.load(Ordering::Acquire),
            written_frames: written,
            durable_frames: u64::MAX,
            dropped_frames: shared.dropped.load(Ordering::Relaxed),
            overflow_blocks: shared.overflows.load(Ordering::Relaxed),
            gap_frames: shared.gaps.load(Ordering::Relaxed),
            invalid_blocks: shared.invalid.load(Ordering::Relaxed),
            clipped_samples: shared.clipped.load(Ordering::Relaxed),
        }
    }
}

/// Single-producer handle. Quiesce all calls before `finish` or drop.
pub struct Recorder {
    config: Config,
    samples: Producer<f64>,
    blocks: Producer<Block>,
    shared: Arc<Shared>,
    next_source: Option<u64>,
    worker: Option<JoinHandle<Result<Report>>>,
}

impl Recorder {
    /// Creates a NEW private directory, prepares all files/buffers and waits for
    /// writer readiness. An error retains any created incomplete evidence.
    pub fn create(directory: &Path, config: Config) -> Result<Self> {
        let max_samples = config.validate()?;
        let capacity = max_samples * config.queue_blocks as usize;
        let (mut samples, mut sample_reader) = RingBuffer::new(capacity);
        let (mut blocks, mut block_reader) = RingBuffer::new(config.queue_blocks as usize);
        // Touch every ring slot during setup, before enabling the producer.
        samples
            .write_chunk_uninit(capacity)
            .unwrap()
            .fill_from_iter(std::iter::repeat_n(0.0, capacity));
        sample_reader.read_chunk(capacity).unwrap().commit_all();
        for _ in 0..config.queue_blocks {
            blocks
                .push(Block {
                    source_frame: 0,
                    frames: 0,
                })
                .unwrap();
        }
        for _ in 0..config.queue_blocks {
            block_reader.pop().unwrap();
        }
        let shared = Arc::new(Shared::default());
        let thread_shared = shared.clone();
        let thread_config = config.clone();
        let directory = directory.to_owned();
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("shr-rec-writer".into())
            .spawn(move || {
                let result = (|| {
                    let files = TakeFiles::create(&directory, &thread_config)?;
                    let scratch = vec![0.0; max_samples];
                    let encoded = vec![0u8; thread_config.max_block as usize * 3];
                    ready_tx.send(()).context("startup receiver disappeared")?;
                    write_take(
                        files,
                        thread_config,
                        sample_reader,
                        block_reader,
                        &thread_shared,
                        scratch,
                        encoded,
                    )
                })();
                if result.is_err() {
                    thread_shared.writer_failed.store(true, Ordering::Release);
                }
                result
            })?;
        if ready_rx.recv().is_err() {
            return match worker.join() {
                Ok(Err(error)) => Err(error),
                _ => Err(anyhow::anyhow!("recorder writer failed during startup")),
            };
        }
        Ok(Self {
            config,
            samples,
            blocks,
            shared,
            next_source: None,
            worker: Some(worker),
        })
    }

    pub fn observer(&self) -> Observer {
        Observer {
            shared: self.shared.clone(),
            epoch: self.config.epoch,
        }
    }

    /// Bounded, allocation/lock/I/O-free. Whole interleaved blocks only.
    /// 0 = accepted, 1 = queue overflow, -1 = invalid/timeline reversal,
    /// -2 = writer fault. Forward gaps are retained and mark the take incomplete.
    pub fn push(&mut self, source_frame: u64, interleaved: &[f64], frames: u32) -> i32 {
        if self.shared.writer_failed.load(Ordering::Acquire) {
            return -2;
        }
        let end = source_frame.checked_add(u64::from(frames));
        if frames == 0
            || frames > self.config.max_block
            || interleaved.len() != frames as usize * self.config.channels as usize
            || end.is_none()
            || self.next_source.is_some_and(|next| source_frame < next)
            || interleaved.iter().any(|x| !x.is_finite())
        {
            self.shared.invalid.fetch_add(1, Ordering::Relaxed);
            return -1;
        }
        if let Some(next) = self.next_source {
            self.shared
                .gaps
                .fetch_add(source_frame - next, Ordering::Relaxed);
        }
        self.next_source = end;
        if self.blocks.slots() == 0 || self.samples.slots() < interleaved.len() {
            self.shared
                .dropped
                .fetch_add(u64::from(frames), Ordering::Relaxed);
            self.shared.overflows.fetch_add(1, Ordering::Relaxed);
            return 1;
        }
        // This producer is exclusive; the consumer can only increase free space.
        // Samples publish first; acquiring the descriptor guarantees visibility.
        self.samples
            .push_entire_slice(interleaved)
            .expect("reserved sample slots");
        self.shared
            .accepted
            .fetch_add(u64::from(frames), Ordering::Release);
        self.blocks
            .push(Block {
                source_frame,
                frames,
            })
            .expect("reserved block slot");
        0
    }

    /// Host faults such as an xrun, disconnect or epoch replacement must be
    /// reported before finish. The first nonzero host-defined code is retained.
    pub fn mark_fault(&self, code: u32) {
        if code != 0 {
            let _ = self.shared.external.compare_exchange(
                0,
                code,
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
        }
    }

    /// Metadata only: this declares source precision, never creates it.
    pub fn set_source_bits(&self, bits: u32) -> Result<()> {
        ensure!((1..=24).contains(&bits), "source valid bits must be 1..24");
        self.shared.source_bits.store(bits, Ordering::Relaxed);
        Ok(())
    }

    pub fn finish(mut self) -> Result<Report> {
        self.join()
    }

    fn join(&mut self) -> Result<Report> {
        self.shared.stopping.store(true, Ordering::Release);
        let result = match self.worker.take() {
            Some(worker) => worker
                .join()
                .map_err(|_| anyhow::anyhow!("recorder writer panicked; retain incomplete take"))
                .and_then(|result| result),
            None => Err(anyhow::anyhow!("already finalized")),
        };
        let outcome = match &result {
            Ok(report) if report.status == "complete" => 1,
            Ok(_) => 2,
            Err(_) => {
                self.shared.writer_failed.store(true, Ordering::Release);
                3
            }
        };
        self.shared.outcome.store(outcome, Ordering::Release);
        result
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        if self.worker.is_some() {
            self.mark_fault(u32::MAX);
            let _ = self.join();
        }
    }
}

struct TakeFiles {
    directory: File,
    stems: Vec<File>,
    timeline: File,
}

fn open_directory(path: &Path) -> Result<File> {
    Ok(OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)?)
}

/// All child file operations are relative to an owned directory descriptor.
fn child(directory: &File, name: &str, create: bool) -> Result<File> {
    let name = CString::new(name)?;
    let flags = libc::O_CLOEXEC
        | libc::O_NOFOLLOW
        | libc::O_NONBLOCK
        | if create {
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL
        } else {
            libc::O_RDONLY
        };
    // SAFETY: valid directory fd/name, returned fd is newly owned here.
    let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags, 0o600) };
    ensure!(
        fd >= 0,
        "open take file: {}",
        std::io::Error::last_os_error()
    );
    let file = unsafe { File::from_raw_fd(fd) };
    ensure!(
        file.metadata()?.is_file(),
        "take member is not a regular file"
    );
    Ok(file)
}

fn stem_name(channel: u32, part: bool) -> String {
    format!(
        "channel-{:03}.wav{}",
        channel + 1,
        if part { ".part" } else { "" }
    )
}

fn publish(directory: &File, from: &str, to: &str) -> Result<()> {
    let from = CString::new(from)?;
    let to = CString::new(to)?;
    // SAFETY: stable owned directory descriptors and valid C strings.
    let rc = unsafe {
        libc::renameat2(
            directory.as_raw_fd(),
            from.as_ptr(),
            directory.as_raw_fd(),
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    ensure!(
        rc == 0,
        "publish take file: {}",
        std::io::Error::last_os_error()
    );
    Ok(())
}

fn write_json(directory: &File, name: &str, value: &impl Serialize) -> Result<()> {
    let mut file = child(directory, name, true)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn wav_header(rate: u32, frames: u64) -> Result<[u8; 44]> {
    ensure!(frames <= MAX_WAV_FRAMES, "mono WAV RIFF size limit");
    let bytes = (frames * 3) as u32;
    let mut header = [0u8; 44];
    header[0..4].copy_from_slice(b"RIFF");
    header[4..8].copy_from_slice(&(36 + bytes + bytes % 2).to_le_bytes());
    header[8..16].copy_from_slice(b"WAVEfmt ");
    header[16..20].copy_from_slice(&16u32.to_le_bytes());
    header[20..22].copy_from_slice(&1u16.to_le_bytes());
    header[22..24].copy_from_slice(&1u16.to_le_bytes());
    header[24..28].copy_from_slice(&rate.to_le_bytes());
    header[28..32].copy_from_slice(&(rate * 3).to_le_bytes());
    header[32..34].copy_from_slice(&3u16.to_le_bytes());
    header[34..36].copy_from_slice(&24u16.to_le_bytes());
    header[36..40].copy_from_slice(b"data");
    header[40..44].copy_from_slice(&bytes.to_le_bytes());
    Ok(header)
}

impl TakeFiles {
    fn create(path: &Path, config: &Config) -> Result<Self> {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(path)
            .context("create new take directory")?;
        let directory = open_directory(path)?;
        // Lock lives on this directory inode through writing/finalization/recovery.
        lock_directory(&directory)?;
        check_space(&directory, MIN_FREE_BYTES)?;
        write_json(
            &directory,
            "session.json",
            &Manifest {
                format: "shr-rec-raw".into(),
                version: 1,
                status: "incomplete".into(),
                encoding: "PCM_S24LE".into(),
                config: config.clone(),
            },
        )?;
        let mut stems = Vec::with_capacity(config.channels as usize);
        for channel in 0..config.channels {
            let mut file = child(&directory, &stem_name(channel, true), true)?;
            file.write_all(&wav_header(config.sample_rate, 0)?)?;
            file.sync_all()?;
            stems.push(file);
        }
        let timeline = child(&directory, "timeline.jsonl", true)?;
        timeline.sync_all()?;
        directory.sync_all()?;
        if let Some(parent) = path.parent() {
            File::open(parent)?.sync_all()?;
        }
        Ok(Self {
            directory,
            stems,
            timeline,
        })
    }

    fn finalize(&mut self, config: &Config, report: &Report) -> Result<()> {
        for file in &mut self.stems {
            if !report.frames.is_multiple_of(2) {
                file.write_all(&[0])?;
            }
            file.seek(SeekFrom::Start(0))?;
            file.write_all(&wav_header(config.sample_rate, report.frames)?)?;
            file.sync_all()?;
        }
        self.timeline.sync_all()?;
        for channel in 0..config.channels {
            publish(
                &self.directory,
                &stem_name(channel, true),
                &stem_name(channel, false),
            )?;
        }
        self.directory.sync_all()?;
        write_json(&self.directory, "result.json.part", report)?;
        publish(&self.directory, "result.json.part", "result.json")?;
        self.directory.sync_all()?;
        Ok(())
    }
}

fn lock_directory(directory: &File) -> Result<()> {
    // SAFETY: flock uses a live fd; no memory arguments.
    ensure!(
        unsafe { libc::flock(directory.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
        "take is still owned by another writer/recovery"
    );
    Ok(())
}

fn check_space(directory: &File, minimum: u64) -> Result<()> {
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: valid live fd and writable correctly sized stat storage.
    ensure!(
        unsafe { libc::fstatvfs(directory.as_raw_fd(), stat.as_mut_ptr()) } == 0,
        "cannot inspect available storage"
    );
    let stat = unsafe { stat.assume_init() };
    ensure!(
        stat.f_bavail.saturating_mul(stat.f_frsize) >= minimum,
        "storage below recording reserve"
    );
    Ok(())
}

fn write_take(
    mut files: TakeFiles,
    config: Config,
    mut samples: Consumer<f64>,
    mut blocks: Consumer<Block>,
    shared: &Shared,
    mut scratch: Vec<f64>,
    mut encoded: Vec<u8>,
) -> Result<Report> {
    let mut frames = 0u64;
    let mut clipped = 0u64;
    let mut written_blocks = 0u64;
    let write_result: Result<()> = (|| {
        loop {
            let block = match blocks.pop() {
                Ok(block) => block,
                Err(_) => {
                    if shared.stopping.load(Ordering::Acquire) {
                        // Producer is quiescent before the release-store; reload
                        // descriptors after acquiring it before deciding drained.
                        if blocks.slots() == 0 {
                            break;
                        }
                        continue;
                    }
                    thread::sleep(Duration::from_micros(250));
                    continue;
                }
            };
            ensure!(
                frames + u64::from(block.frames) <= MAX_WAV_FRAMES,
                "mono WAV RIFF size limit"
            );
            if written_blocks.is_multiple_of(256) {
                check_space(&files.directory, MIN_FREE_BYTES)?;
            }
            let count = block.frames as usize * config.channels as usize;
            samples
                .pop_entire_slice(&mut scratch[..count])
                .map_err(|_| anyhow::anyhow!("queue contract violated"))?;
            for (channel, file) in files.stems.iter_mut().enumerate() {
                for frame in 0..block.frames as usize {
                    let sample = scratch[frame * config.channels as usize + channel];
                    let quantized = (sample * 8_388_608.0).round();
                    if !(-8_388_608.0..=8_388_607.0).contains(&quantized) {
                        clipped += 1;
                        shared.clipped.fetch_add(1, Ordering::Relaxed);
                    }
                    let bytes = (quantized.clamp(-8_388_608.0, 8_388_607.0) as i32).to_le_bytes();
                    encoded[frame * 3..frame * 3 + 3].copy_from_slice(&bytes[..3]);
                }
                file.write_all(&encoded[..block.frames as usize * 3])?;
            }
            serde_json::to_writer(
                &mut files.timeline,
                &Timeline {
                    source_frame: block.source_frame,
                    frames: block.frames,
                    file_frame: frames,
                },
            )?;
            files.timeline.write_all(b"\n")?;
            frames += u64::from(block.frames);
            shared.written.store(frames, Ordering::Release);
            written_blocks += 1;
        }
        Ok(())
    })();
    let bits = shared.source_bits.load(Ordering::Relaxed);
    let mut report = Report {
        status: "incomplete".into(),
        frames,
        epoch: config.epoch,
        source_valid_bits: (bits != 0).then_some(bits),
        dropped_frames: shared.dropped.load(Ordering::Relaxed),
        overflow_blocks: shared.overflows.load(Ordering::Relaxed),
        source_gap_frames: shared.gaps.load(Ordering::Relaxed),
        invalid_blocks: shared.invalid.load(Ordering::Relaxed),
        external_fault: shared.external.load(Ordering::Relaxed),
        clipped_samples: clipped,
        error: write_result.as_ref().err().map(ToString::to_string),
    };
    if write_result.is_err() {
        shared.writer_failed.store(true, Ordering::Release);
        // Never repair/truncate original files following an I/O error.
        let _ = write_json(&files.directory, "failure.json", &report);
        return write_result.map(|()| report);
    }
    if frames > 0
        && report.dropped_frames == 0
        && report.source_gap_frames == 0
        && report.invalid_blocks == 0
        && report.external_fault == 0
        && clipped == 0
    {
        report.status = "complete".into();
    }
    if let Err(error) = files.finalize(&config, &report) {
        shared.writer_failed.store(true, Ordering::Release);
        report.status = "incomplete".into();
        report.error = Some(error.to_string());
        let _ = write_json(&files.directory, "failure.json", &report);
        return Err(error);
    }
    Ok(report)
}

/// Create off callback while the producer is quiesced. Observer owns only atomics.
/// # Safety
/// `handle` must be live and exclusively available for this setup call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn shr_rec_v2_observer_create(handle: *mut Recorder) -> *mut Observer {
    match unsafe { handle.as_ref() } {
        Some(recorder) => Box::into_raw(Box::new(recorder.observer())),
        None => std::ptr::null_mut(),
    }
}

/// # Safety
/// Observer must remain live during this off-callback call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn shr_rec_v2_observer_retain(observer: *const Observer) -> *mut Observer {
    match unsafe { observer.as_ref() } {
        Some(observer) => Box::into_raw(Box::new(observer.clone())),
        None => std::ptr::null_mut(),
    }
}

/// # Safety
/// Consumes one observer handle off callback; no use may race its release.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn shr_rec_v2_observer_release(observer: *mut Observer) {
    if !observer.is_null() {
        drop(unsafe { Box::from_raw(observer) });
    }
}

/// # Safety
/// Observer must be live; output must be writable, aligned and unaliased for
/// `output_size` bytes. Never release this observer while querying it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn shr_rec_v2_observer_snapshot(
    observer: *const Observer,
    output: *mut Progress,
    output_size: u32,
) -> i32 {
    if observer.is_null()
        || output.is_null()
        || output_size as usize != std::mem::size_of::<Progress>()
    {
        return -1;
    }
    unsafe { output.write((&*observer).snapshot()) };
    0
}

/// Copy validated common whole frames to a NEW destination. Source files are
/// descriptor-held and never repaired, truncated, renamed or overwritten.
/// This native-v1 recovery deliberately never labels a crash take complete.
pub fn recover(source: &Path, destination: &Path) -> Result<Report> {
    let directory = open_directory(source)?;
    lock_directory(&directory)?;
    let mut manifest_file = child(&directory, "session.json", false)?;
    ensure!(
        manifest_file.metadata()?.len() <= 16_384,
        "oversize manifest"
    );
    let mut json = Vec::new();
    manifest_file.read_to_end(&mut json)?;
    let manifest: Manifest = serde_json::from_slice(&json)?;
    ensure!(
        manifest.format == "shr-rec-raw"
            && manifest.version == 1
            && manifest.encoding == "PCM_S24LE"
            && manifest.status == "incomplete",
        "unsupported native take"
    );
    manifest.config.validate()?;
    let mut inputs = Vec::new();
    let mut frames = MAX_WAV_FRAMES;
    for channel in 0..manifest.config.channels {
        let a = child(&directory, &stem_name(channel, true), false);
        let b = child(&directory, &stem_name(channel, false), false);
        // Inspect both names without following symlinks; errors other than absent
        // cannot be silently treated as the unselected side of publication.
        let exists = |name: String| -> Result<bool> {
            let name = CString::new(name)?;
            let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
            let rc = unsafe {
                libc::fstatat(
                    directory.as_raw_fd(),
                    name.as_ptr(),
                    stat.as_mut_ptr(),
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            };
            if rc == 0 {
                Ok(true)
            } else {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::NotFound {
                    Ok(false)
                } else {
                    Err(error.into())
                }
            }
        };
        let part = exists(stem_name(channel, true))?;
        ensure!(
            part != exists(stem_name(channel, false))?,
            "ambiguous or missing stem"
        );
        let mut file = if part { a? } else { b? };
        let size = file.metadata()?.len();
        ensure!(
            (44..=44 + MAX_WAV_FRAMES * 3 + 1).contains(&size),
            "invalid stem size"
        );
        let mut header = [0u8; 44];
        file.read_exact(&mut header)?;
        let expected = wav_header(manifest.config.sample_rate, 0)?;
        ensure!(
            header[..4] == expected[..4] && header[8..40] == expected[8..40],
            "invalid stem header/rate/encoding"
        );
        let header_bytes = u32::from_le_bytes(header[40..44].try_into().unwrap()) as u64;
        let available = if header_bytes > 0 {
            ensure!(
                header_bytes.is_multiple_of(3) && header_bytes <= size - 44,
                "invalid finalized stem length"
            );
            header_bytes / 3
        } else {
            (size - 44) / 3
        };
        frames = frames.min(available);
        inputs.push(file);
    }
    // All source stems validated before destination creation; originals retained.
    let mut output = TakeFiles::create(destination, &manifest.config)?;
    for (input, stem) in inputs.iter_mut().zip(&mut output.stems) {
        let copied = std::io::copy(&mut input.take(frames * 3), stem)?;
        ensure!(copied == frames * 3, "source changed during recovery");
    }
    let report = Report {
        status: "recovered-incomplete".into(),
        frames,
        epoch: manifest.config.epoch,
        source_valid_bits: None,
        dropped_frames: 0,
        overflow_blocks: 0,
        source_gap_frames: 0,
        invalid_blocks: 0,
        external_fault: 0,
        clipped_samples: 0,
        error: Some(
            "Crash recovery: original evidence retained; source timeline/counters require review"
                .into(),
        ),
    };
    output.finalize(&manifest.config, &report)?;
    Ok(report)
}

/// # Safety
/// `directory` must point to a valid NUL-terminated UTF-8 path for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn shr_rec_v1_create(
    directory: *const c_char,
    sample_rate: u32,
    channels: u32,
    max_block: u32,
    queue_blocks: u32,
    epoch: u64,
) -> *mut Recorder {
    if directory.is_null() {
        return std::ptr::null_mut();
    }
    let result = std::panic::catch_unwind(|| {
        let path = unsafe { CStr::from_ptr(directory) }.to_str().ok()?;
        Recorder::create(
            Path::new(path),
            Config {
                sample_rate,
                channels,
                max_block,
                queue_blocks,
                epoch,
            },
        )
        .ok()
    });
    match result {
        Ok(Some(recorder)) => Box::into_raw(Box::new(recorder)),
        _ => std::ptr::null_mut(),
    }
}

/// # Safety
/// Handle must be live and exclusively owned by this producer. `interleaved`
/// must contain `frames * channels` doubles; never race finish or another push.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn shr_rec_v1_push(
    handle: *mut Recorder,
    source_frame: u64,
    interleaved: *const f64,
    frames: u32,
) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let recorder = unsafe { &mut *handle };
    if interleaved.is_null() || frames == 0 || frames > recorder.config.max_block {
        recorder.shared.invalid.fetch_add(1, Ordering::Relaxed);
        return -1;
    }
    let samples = unsafe {
        std::slice::from_raw_parts(
            interleaved,
            frames as usize * recorder.config.channels as usize,
        )
    };
    recorder.push(source_frame, samples, frames)
}

/// # Safety
/// Handle must be live. Do not race its destruction.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn shr_rec_v1_mark_fault(handle: *mut Recorder, code: u32) {
    if let Some(recorder) = unsafe { handle.as_ref() } {
        recorder.mark_fault(code);
    }
}

/// # Safety
/// Handle must be live. Call during setup before callback activation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn shr_rec_v1_set_source_bits(handle: *mut Recorder, bits: u32) -> i32 {
    match unsafe { handle.as_ref() } {
        Some(recorder) if recorder.set_source_bits(bits).is_ok() => 0,
        _ => -1,
    }
}

/// # Safety
/// Takes ownership of exactly one live handle. Producer must be quiescent.
/// Returns 0 only for a complete take; 1 for finalized incomplete, -1 for error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn shr_rec_v1_finish(handle: *mut Recorder) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        unsafe { Box::from_raw(handle) }.finish()
    }));
    match result {
        Ok(Ok(report)) if report.status == "complete" => 0,
        Ok(Ok(_)) => 1,
        _ => -1,
    }
}

/// # Safety
/// Both paths must be valid NUL-terminated UTF-8 strings for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn shr_rec_v1_recover(
    source: *const c_char,
    destination: *const c_char,
) -> i32 {
    if source.is_null() || destination.is_null() {
        return -1;
    }
    let result = std::panic::catch_unwind(|| -> Result<()> {
        let source = unsafe { CStr::from_ptr(source) }.to_str()?;
        let destination = unsafe { CStr::from_ptr(destination) }.to_str()?;
        recover(Path::new(source), Path::new(destination))?;
        Ok(())
    });
    if matches!(result, Ok(Ok(()))) { 1 } else { -1 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;

    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "shr-rec-engine-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    fn config() -> Config {
        Config {
            sample_rate: 48_000,
            channels: 2,
            max_block: 192,
            queue_blocks: 8,
            epoch: 71,
        }
    }
    fn decode(path: &Path) -> Vec<i32> {
        let data = fs::read(path).unwrap();
        let size = u32::from_le_bytes(data[40..44].try_into().unwrap()) as usize;
        assert_eq!(data.len(), 44 + size + size % 2);
        assert_eq!(
            u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize + 8,
            data.len()
        );
        data[44..44 + size]
            .chunks_exact(3)
            .map(|s| i32::from_le_bytes([s[0], s[1], s[2], if s[2] & 128 != 0 { 255 } else { 0 }]))
            .collect()
    }

    #[test]
    fn observer_survives_complete_incomplete_error_and_drop() {
        let temp = Temp::new();
        for (name, expected) in [("complete", 1), ("fault", 2), ("error", 3), ("drop", 2)] {
            let path = temp.path(name);
            let mut recorder = Recorder::create(&path, config()).unwrap();
            let observer = recorder.observer();
            let retained = observer.clone();
            assert_eq!(observer.snapshot().outcome, 0);
            assert_eq!(recorder.push(48048, &[0.25, -0.25], 1), 0);
            if name == "fault" {
                recorder.mark_fault(17);
            }
            if name == "error" {
                fs::write(path.join("channel-002.wav"), b"collision").unwrap();
            }
            if name == "drop" {
                drop(recorder);
            } else {
                let result = recorder.finish();
                assert_eq!(result.is_err(), name == "error");
            }
            drop(observer);
            let status = retained.snapshot();
            assert_eq!(status.state, 3);
            assert_eq!(status.outcome, expected);
            assert_eq!(status.accepted_frames, 1);
            assert_eq!(status.written_frames, 1);
            assert_eq!(status.durable_frames, u64::MAX);
            assert_eq!(status.writer_fault, u32::from(name == "error"));
        }
    }

    #[test]
    fn observer_queries_concurrently_with_exclusive_producer() {
        let temp = Temp::new();
        let mut recorder = Recorder::create(&temp.path("concurrent"), config()).unwrap();
        let observer = recorder.observer();
        let query = observer.clone();
        let reader = thread::spawn(move || {
            loop {
                let status = query.snapshot();
                assert!(status.written_frames <= status.accepted_frames);
                if status.outcome != 0 {
                    assert_eq!(status.state, 3);
                    assert_eq!(status.accepted_frames, status.written_frames);
                    return status;
                }
                thread::yield_now();
            }
        });
        for period in 0..1000 {
            let _ = recorder.push(period * 192, &[0.0; 384], 192);
        }
        recorder.finish().unwrap();
        let status = reader.join().unwrap();
        assert_eq!(status.accepted_frames + status.dropped_frames, 192000);
        assert_eq!(observer.snapshot().written_frames, status.written_frames);
    }

    #[test]
    fn observer_join_panic_publishes_error() {
        let temp = Temp::new();
        let mut recorder = Recorder::create(&temp.path("panic"), config()).unwrap();
        let observer = recorder.observer();
        recorder.shared.stopping.store(true, Ordering::Release);
        recorder.worker.take().unwrap().join().unwrap().unwrap();
        recorder.worker = Some(thread::spawn(|| panic!("synthetic writer panic")));
        assert!(recorder.finish().is_err());
        let status = observer.snapshot();
        assert_eq!(
            (status.state, status.outcome, status.writer_fault),
            (3, 3, 1)
        );
    }

    #[test]
    fn abi_observer_bounds_lifetime_and_samples() {
        let temp = Temp::new();
        let path = temp.path("abi");
        let cpath = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
        unsafe {
            let handle = shr_rec_v1_create(cpath.as_ptr(), 48000, 2, 48, 8, 9);
            assert!(!handle.is_null());
            let observer = shr_rec_v2_observer_create(handle);
            let retained = shr_rec_v2_observer_retain(observer);
            let mut progress = Progress {
                version: 123,
                ..Progress::default()
            };
            let size = std::mem::size_of::<Progress>() as u32;
            assert_eq!(size, 96);
            assert_eq!(
                shr_rec_v2_observer_snapshot(observer, &mut progress, size - 1),
                -1
            );
            assert_eq!(progress.version, 123);
            assert_eq!(
                shr_rec_v2_observer_snapshot(std::ptr::null(), &mut progress, size),
                -1
            );
            assert_eq!(progress.version, 123);
            assert_eq!(
                shr_rec_v2_observer_snapshot(observer, std::ptr::null_mut(), size),
                -1
            );
            let mut samples = [0.0; 96];
            for frame in 0..48 {
                samples[frame * 2] = (frame as f64 + 1.0) / 8388608.0;
                samples[frame * 2 + 1] = -(frame as f64 + 1.0) / 8388608.0;
            }
            for block in 0..2 {
                assert_eq!(
                    shr_rec_v1_push(handle, 48048 + block * 48, samples.as_ptr(), 48),
                    0
                );
                assert_eq!(
                    shr_rec_v2_observer_snapshot(observer, &mut progress, size),
                    0
                );
                assert!(progress.written_frames <= progress.accepted_frames);
                assert_eq!(progress.outcome, 0);
            }
            shr_rec_v2_observer_release(observer);
            assert_eq!(shr_rec_v1_finish(handle), 0);
            assert_eq!(
                shr_rec_v2_observer_snapshot(retained, &mut progress, size),
                0
            );
            assert_eq!(
                (
                    progress.version,
                    progress.state,
                    progress.outcome,
                    progress.epoch
                ),
                (2, 3, 1, 9)
            );
            assert_eq!(
                (progress.accepted_frames, progress.written_frames),
                (96, 96)
            );
            assert_eq!(progress.reserved, 0);
            shr_rec_v2_observer_release(retained);
            let expected: Vec<i32> = (1..=48).cycle().take(96).collect();
            assert_eq!(decode(&path.join("channel-001.wav")), expected);
            assert_eq!(
                decode(&path.join("channel-002.wav")),
                expected.iter().map(|x| -x).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn preserves_pcm24_extremes_channel_order_timeline_and_odd_padding() {
        let temp = Temp::new();
        let path = temp.path("take");
        let mut recorder = Recorder::create(&path, config()).unwrap();
        recorder.set_source_bits(24).unwrap();
        let codes = [-8_388_608, -7_654_321, -1, 0, 1, 5_678_901, 8_388_607];
        let mut samples = Vec::new();
        for code in codes {
            samples.push(f64::from(code) / 8_388_608.0);
            samples.push(f64::from(-code - 1) / 8_388_608.0);
        }
        assert_eq!(recorder.push(123, &samples[..6], 3), 0);
        assert_eq!(recorder.push(126, &samples[6..], 4), 0);
        let report = recorder.finish().unwrap();
        assert_eq!(report.status, "complete");
        assert_eq!(report.source_valid_bits, Some(24));
        assert_eq!(report.frames, 7);
        assert_eq!(decode(&path.join("channel-001.wav")), codes);
        assert_eq!(decode(&path.join("channel-002.wav")), codes.map(|x| -x - 1));
        let journal = fs::read_to_string(path.join("timeline.jsonl")).unwrap();
        let entries: Vec<Timeline> = journal
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!((entries[0].source_frame, entries[0].file_frame), (123, 0));
        assert_eq!((entries[1].source_frame, entries[1].file_frame), (126, 3));
    }

    #[test]
    fn forward_gap_invalid_source_and_external_fault_never_complete() {
        let temp = Temp::new();
        let mut recorder = Recorder::create(&temp.path("take"), config()).unwrap();
        let observer = recorder.observer();
        assert_eq!(recorder.push(0, &[0.0; 4], 2), 0);
        assert_eq!(recorder.push(8, &[0.0; 4], 2), 0);
        assert_eq!(recorder.push(9, &[0.0; 4], 2), -1);
        assert_eq!(recorder.push(10, &[f64::NAN; 4], 2), -1);
        assert_eq!(recorder.push(u64::MAX, &[0.0; 4], 2), -1);
        recorder.mark_fault(4);
        recorder.mark_fault(5);
        let report = recorder.finish().unwrap();
        let status = observer.snapshot();
        assert_eq!(
            (status.gap_frames, status.invalid_blocks, status.host_fault),
            (6, 3, 4)
        );
        assert_eq!(
            (
                status.accepted_frames,
                status.written_frames,
                status.outcome
            ),
            (4, 4, 2)
        );
        assert_eq!(report.status, "incomplete");
        assert_eq!(
            (
                report.source_gap_frames,
                report.invalid_blocks,
                report.external_fault
            ),
            (6, 3, 4)
        );
        assert_eq!(report.frames, 4);
    }

    #[test]
    fn overflow_rejects_whole_period_and_marks_incomplete() {
        let temp = Temp::new();
        let config = Config {
            queue_blocks: 2,
            max_block: 2,
            ..config()
        };
        let (samples, sample_reader) = RingBuffer::new(8);
        let (blocks, block_reader) = RingBuffer::new(2);
        let shared = Arc::new(Shared::default());
        let mut recorder = Recorder {
            config: config.clone(),
            samples,
            blocks,
            shared: shared.clone(),
            next_source: None,
            worker: None,
        };
        assert_eq!(recorder.push(100, &[0.0; 4], 2), 0);
        assert_eq!(recorder.push(102, &[0.0; 4], 2), 0);
        assert_eq!(recorder.push(104, &[0.0; 4], 2), 1);
        shared.stopping.store(true, Ordering::Release);
        let files = TakeFiles::create(&temp.path("take"), &config).unwrap();
        let report = write_take(
            files,
            config,
            sample_reader,
            block_reader,
            &shared,
            vec![0.0; 4],
            vec![0; 6],
        )
        .unwrap();
        assert_eq!(
            (report.frames, report.dropped_frames, report.overflow_blocks),
            (4, 2, 1)
        );
        assert_eq!(report.status, "incomplete");
    }

    #[test]
    fn startup_collisions_empty_capture_drop_and_clipping_remain_incomplete() {
        let temp = Temp::new();
        fs::write(temp.path("existing"), b"retain").unwrap();
        assert!(Recorder::create(&temp.path("existing"), config()).is_err());
        assert_eq!(fs::read(temp.path("existing")).unwrap(), b"retain");
        let recorder = Recorder::create(&temp.path("empty"), config()).unwrap();
        assert_eq!(recorder.finish().unwrap().status, "incomplete");
        let mut recorder = Recorder::create(&temp.path("clip"), config()).unwrap();
        let observer = recorder.observer();
        assert_eq!(recorder.push(0, &[2.0, -2.0], 1), 0);
        let report = recorder.finish().unwrap();
        assert_eq!(
            (report.status.as_str(), report.clipped_samples),
            ("incomplete", 2)
        );
        assert_eq!(observer.snapshot().clipped_samples, 2);
        let mut recorder = Recorder::create(&temp.path("drop"), config()).unwrap();
        assert_eq!(recorder.push(0, &[0.0; 2], 1), 0);
        drop(recorder);
        let report: Report =
            serde_json::from_slice(&fs::read(temp.path("drop/result.json")).unwrap()).unwrap();
        assert_eq!(report.status, "incomplete");
        assert_eq!(report.external_fault, u32::MAX);
    }

    #[test]
    fn storage_write_and_publication_failures_preserve_incomplete_evidence() {
        let temp = Temp::new();
        let mut files = TakeFiles::create(&temp.path("disk"), &config()).unwrap();
        assert!(check_space(&files.directory, u64::MAX).is_err());
        files.stems[1] = OpenOptions::new().write(true).open("/dev/full").unwrap();
        let (mut samples, sample_reader) = RingBuffer::new(2);
        let (mut blocks, block_reader) = RingBuffer::new(2);
        samples.push_entire_slice(&[0.0; 2]).unwrap();
        blocks
            .push(Block {
                source_frame: 0,
                frames: 1,
            })
            .unwrap();
        let shared = Shared::default();
        shared.stopping.store(true, Ordering::Release);
        assert!(
            write_take(
                files,
                config(),
                sample_reader,
                block_reader,
                &shared,
                vec![0.0; 384],
                vec![0; 576]
            )
            .is_err()
        );
        assert!(shared.writer_failed.load(Ordering::Acquire));
        assert!(!temp.path("disk/result.json").exists());
        assert!(temp.path("disk/failure.json").exists());
        assert!(temp.path("disk/channel-001.wav.part").exists());

        let mut recorder = Recorder::create(&temp.path("collision"), config()).unwrap();
        fs::write(temp.path("collision/channel-002.wav"), b"retain").unwrap();
        assert_eq!(recorder.push(0, &[0.0; 2], 1), 0);
        assert!(recorder.finish().is_err());
        assert_eq!(
            fs::read(temp.path("collision/channel-002.wav")).unwrap(),
            b"retain"
        );
        assert!(temp.path("collision/failure.json").exists());
        assert!(!temp.path("collision/result.json").exists());
    }

    fn interrupted(path: &Path, rename_count: u32) {
        let mut files = TakeFiles::create(path, &config()).unwrap();
        for (index, file) in files.stems.iter_mut().enumerate() {
            for code in 0..(7 + index) as i32 {
                file.write_all(&code.to_le_bytes()[..3]).unwrap();
            }
        }
        for channel in 0..rename_count {
            publish(
                &files.directory,
                &stem_name(channel, true),
                &stem_name(channel, false),
            )
            .unwrap();
        }
    }

    #[test]
    fn crash_recovery_handles_every_partial_publication_without_mutating_source() {
        let temp = Temp::new();
        for renamed in 0..=2 {
            let source = temp.path(&format!("source-{renamed}"));
            interrupted(&source, renamed);
            let original: Vec<_> = (0..2)
                .map(|c| fs::read(source.join(stem_name(c, c >= renamed))).unwrap())
                .collect();
            for retry in 0..2 {
                let dest = temp.path(&format!("recover-{renamed}-{retry}"));
                let report = recover(&source, &dest).unwrap();
                assert_eq!(
                    (report.status.as_str(), report.frames),
                    ("recovered-incomplete", 7)
                );
                assert_eq!(
                    decode(&dest.join("channel-001.wav")),
                    (0..7).collect::<Vec<_>>()
                );
                assert_eq!(
                    decode(&dest.join("channel-002.wav")),
                    (0..7).collect::<Vec<_>>()
                );
                assert!(recover(&source, &dest).is_err());
            }
            for channel in 0..2 {
                assert_eq!(
                    fs::read(source.join(stem_name(channel, channel >= renamed))).unwrap(),
                    original[channel as usize]
                );
            }
        }
    }

    #[test]
    fn recovery_refuses_live_owner_symlink_ambiguity_future_schema_and_bad_rate() {
        let temp = Temp::new();
        let live = Recorder::create(&temp.path("live"), config()).unwrap();
        assert!(recover(&temp.path("live"), &temp.path("live-copy")).is_err());
        drop(live);
        for mode in ["symlink", "both", "future", "rate"] {
            let source = temp.path(mode);
            interrupted(&source, 0);
            match mode {
                "symlink" => {
                    fs::remove_file(source.join("channel-002.wav.part")).unwrap();
                    symlink("channel-001.wav.part", source.join("channel-002.wav.part")).unwrap();
                }
                "both" => {
                    fs::copy(
                        source.join("channel-002.wav.part"),
                        source.join("channel-002.wav"),
                    )
                    .unwrap();
                }
                "future" => {
                    let path = source.join("session.json");
                    let mut value: serde_json::Value =
                        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                    value["version"] = 2.into();
                    fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
                }
                "rate" => {
                    let mut file = OpenOptions::new()
                        .write(true)
                        .open(source.join("channel-002.wav.part"))
                        .unwrap();
                    file.seek(SeekFrom::Start(24)).unwrap();
                    file.write_all(&44_100u32.to_le_bytes()).unwrap();
                }
                _ => unreachable!(),
            }
            let before = fs::read(source.join("channel-001.wav.part")).unwrap();
            let dest = temp.path(&format!("{mode}-copy"));
            assert!(recover(&source, &dest).is_err());
            assert!(!dest.exists());
            assert_eq!(
                fs::read(source.join("channel-001.wav.part")).unwrap(),
                before
            );
        }
    }

    #[test]
    fn riff_limit_is_explicit_and_does_not_wrap() {
        assert!(wav_header(48_000, MAX_WAV_FRAMES).is_ok());
        assert!(wav_header(48_000, MAX_WAV_FRAMES + 1).is_err());
        assert!(
            Config {
                channels: 65,
                ..config()
            }
            .validate()
            .is_err()
        );
        assert!(
            Config {
                channels: 64,
                max_block: 8192,
                queue_blocks: 4096,
                ..config()
            }
            .validate()
            .is_err()
        );
    }
}
