#![allow(dead_code)]
use std::{
    fs,
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};
static SERIAL: AtomicUsize = AtomicUsize::new(0);
pub struct Scratch(pub PathBuf);
impl Scratch {
    pub fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "rec-player-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
pub fn sample(stem: usize, frame: u64) -> i32 {
    if stem.is_multiple_of(2) {
        (frame % 1000) as i32 + 1 + stem as i32 * 1000
    } else {
        -((frame % 1000) as i32 + 1 + stem as i32 * 1000)
    }
}
pub fn wav(path: &Path, rate: u32, frames: u64, stem: usize) {
    let bytes = (frames * 3) as u32;
    let pad = bytes % 2;
    let mut f = std::io::BufWriter::new(fs::File::create(path).unwrap());
    f.write_all(b"RIFF").unwrap();
    f.write_all(&(36 + bytes + pad).to_le_bytes()).unwrap();
    f.write_all(b"WAVEfmt ").unwrap();
    f.write_all(&16u32.to_le_bytes()).unwrap();
    f.write_all(&1u16.to_le_bytes()).unwrap();
    f.write_all(&1u16.to_le_bytes()).unwrap();
    f.write_all(&rate.to_le_bytes()).unwrap();
    f.write_all(&(rate * 3).to_le_bytes()).unwrap();
    f.write_all(&3u16.to_le_bytes()).unwrap();
    f.write_all(&24u16.to_le_bytes()).unwrap();
    f.write_all(b"data").unwrap();
    f.write_all(&bytes.to_le_bytes()).unwrap();
    for i in 0..frames {
        f.write_all(&sample(stem, i).to_le_bytes()[..3]).unwrap();
    }
    if pad != 0 {
        f.write_all(&[0]).unwrap();
    }
}
pub fn shr(root: &Path, frames: u64, count: usize) -> PathBuf {
    let path = root.join("shr.take");
    fs::create_dir(&path).unwrap();
    let tracks:Vec<_>=(0..count).map(|i| {let name=format!("stem-{i}.wav");wav(&path.join(&name),48000,frames,i);serde_json::json!({"id":format!("id-{i}"),"label":format!("Stem {i}"),"group":null,"role":"mono","preferred_source":format!("unrelated:input-{i}"),"actual_jack_port":format!("unrelated:input-{i}"),"wav_file":name,"frames":frames,"finalized":true})}).collect();
    let value = serde_json::json!({"format_version":1,"session_name":"Synthetic band","sample_rate":48000,"total_frames":frames,"duration_seconds":frames as f64/48000.,"completeness":"complete","finalization":"finalized","dropped_frames":0,"overflow_events":0,"callback_violations":0,"xruns":0,"writer_high_water_frames":0,"recovery":[],"tracks":tracks});
    fs::write(
        path.join("session.json"),
        serde_json::to_vec_pretty(&value).unwrap(),
    )
    .unwrap();
    path
}
pub fn edit(path: &Path, change: impl FnOnce(&mut serde_json::Value)) {
    let p = path.join("session.json");
    let mut v = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
    change(&mut v);
    fs::write(p, serde_json::to_vec(&v).unwrap()).unwrap();
}
pub fn exact(count: usize) -> shr_rec::playback::Routing {
    shr_rec::playback::Routing::Exact {
        outputs: (0..count).map(|i| format!("sink:out-{i}")).collect(),
    }
}
pub fn overwrite_sample(path: &Path, frame: u64, value: i32) {
    let mut f = fs::OpenOptions::new().write(true).open(path).unwrap();
    f.seek(SeekFrom::Start(44 + frame * 3)).unwrap();
    f.write_all(&value.to_le_bytes()[..3]).unwrap();
}
