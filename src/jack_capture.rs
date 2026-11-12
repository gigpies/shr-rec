//! Capture shares the existing no-start dynamic JACK boundary. Only explicit
//! start connects the selected exact sources. The producer is callback-owned.
use super::*;
use crate::recorder::Recorder;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

pub const MAX_PERIOD: usize = 8192;
pub const ROUTE_LOST: u32 = 1;
pub const DEVICE_LOST: u32 = 2;
pub const RATE_CHANGED: u32 = 3;
pub const XRUN: u32 = 4;
pub const INVALID_PERIOD: u32 = 5;
pub const PUSH_FAILED: u32 = 6;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub name: String,
    pub uuid: u64,
}
#[derive(Clone, Debug)]
pub struct Inputs {
    pub rate: u32,
    pub sources: Vec<Source>,
}
pub struct SharedCapture {
    enabled: AtomicBool,
    fault: AtomicU32,
    closing: AtomicBool,
    peaks: [AtomicU32; 64],
}
impl Default for SharedCapture {
    fn default() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            fault: AtomicU32::new(0),
            closing: AtomicBool::new(false),
            peaks: std::array::from_fn(|_| AtomicU32::new(0)),
        }
    }
}
impl SharedCapture {
    pub fn fault(&self, code: u32) {
        if self.closing.load(Ordering::Acquire) {
            return;
        }
        let _ = self
            .fault
            .compare_exchange(0, code, Ordering::AcqRel, Ordering::Relaxed);
        self.enabled.store(false, Ordering::Release);
    }
    pub fn peak(&self, index: usize) -> f32 {
        self.peaks
            .get(index)
            .map_or(0., |p| f32::from_bits(p.load(Ordering::Relaxed)))
    }
    pub fn failure(&self) -> u32 {
        self.fault.load(Ordering::Acquire)
    }
    pub fn enable(&self) {
        if self.failure() != 0 {
            return;
        }
        self.enabled.store(true, Ordering::Release);
    }
    pub fn disable(&self) {
        self.enabled.store(false, Ordering::Release);
    }
}
/// The adapter must always return ownership of the writer after quiescence,
/// including startup failure. No adapter may destroy it on the callback thread.
pub trait Device {
    fn inputs(&mut self) -> Result<Inputs>;
    fn attach(
        &mut self,
        recorder: Recorder,
        sources: &[Source],
        rate: u32,
    ) -> std::result::Result<Arc<SharedCapture>, (Box<Recorder>, String)>;
    fn poll(&mut self) {}
    fn quiesce(&mut self) -> Option<Recorder>;
}
#[derive(Default)]
pub struct JackCapture {
    running: Option<CaptureRunning>,
}
struct CaptureCallback {
    recorder: Option<Recorder>,
    shared: Arc<SharedCapture>,
    ports: Vec<*mut c_void>,
    buffer: GetBuffer,
    scratch: Vec<f64>,
    frame: u64,
}
unsafe extern "C" fn capture_process(frames: c_uint, data: *mut c_void) -> c_int {
    let c = unsafe { &mut *data.cast::<CaptureCallback>() };
    if frames == 0 || frames as usize > MAX_PERIOD {
        c.shared.fault(INVALID_PERIOD);
        return 1;
    }
    if !c.shared.enabled.load(Ordering::Acquire) || c.shared.failure() != 0 {
        return 0;
    }
    let mut inputs = [ptr::null::<f32>(); 64];
    for (index, port) in c.ports.iter().enumerate() {
        inputs[index] = unsafe { (c.buffer)(*port, frames) }.cast();
        if inputs[index].is_null() {
            c.shared.fault(DEVICE_LOST);
            return 0;
        }
    }
    let count = c.ports.len();
    let mut peaks = [0f32; 64];
    for frame in 0..frames as usize {
        for (channel, input) in inputs[..count].iter().enumerate() {
            let value = unsafe { *input.add(frame) };
            peaks[channel] = peaks[channel].max(value.abs());
            c.scratch[frame * count + channel] = f64::from(value);
        }
    }
    for (index, peak) in peaks[..count].iter().enumerate() {
        c.shared.peaks[index].store(peak.to_bits(), Ordering::Relaxed);
    }
    let recorder = c.recorder.as_mut().expect("live producer");
    if recorder.push(c.frame, &c.scratch[..frames as usize * count], frames) != 0 {
        c.shared.fault(PUSH_FAILED);
    }
    c.frame += u64::from(frames);
    0
}
struct CaptureNotifications {
    shared: Arc<SharedCapture>,
    rate: u32,
    edges: Vec<(u32, u32)>,
}
unsafe extern "C" fn capture_graph(a: c_uint, b: c_uint, connected: c_int, data: *mut c_void) {
    let n = unsafe { &*data.cast::<CaptureNotifications>() };
    if connected == 0
        && n.edges
            .iter()
            .any(|&(x, y)| (a == x && b == y) || (a == y && b == x))
    {
        n.shared.fault(ROUTE_LOST);
    }
}
unsafe extern "C" fn capture_shutdown(data: *mut c_void) {
    unsafe { &*data.cast::<CaptureNotifications>() }
        .shared
        .fault(DEVICE_LOST);
}
unsafe extern "C" fn capture_rate(rate: c_uint, data: *mut c_void) -> c_int {
    let n = unsafe { &*data.cast::<CaptureNotifications>() };
    if rate != n.rate {
        n.shared.fault(RATE_CHANGED);
    }
    0
}
unsafe extern "C" fn capture_xrun(data: *mut c_void) -> c_int {
    unsafe { &*data.cast::<CaptureNotifications>() }
        .shared
        .fault(XRUN);
    0
}
struct CaptureRunning {
    client: Option<Client>,
    callback: Box<UnsafeCell<CaptureCallback>>,
    notifications: Box<CaptureNotifications>,
    sources: Vec<(CString, u64)>,
    ports: Vec<*mut c_void>,
}
impl CaptureRunning {
    fn quiesce(&mut self) -> Option<Recorder> {
        self.notifications.shared.disable();
        if let Some(client) = &mut self.client
            && client.active
        {
            if unsafe { (client.api.deactivate)(client.client) } != 0 {
                self.notifications.shared.fault(DEVICE_LOST);
            }
            client.active = false;
        }
        self.notifications
            .shared
            .closing
            .store(true, Ordering::Release);
        drop(self.client.take()); // close is the callback lifetime barrier
        let callback = self.callback.get_mut();
        let recorder = callback.recorder.take();
        if let Some(r) = &recorder {
            let fault = self.notifications.shared.failure();
            if fault != 0 {
                r.mark_fault(fault);
            }
        }
        recorder
    }
}
impl Drop for CaptureRunning {
    fn drop(&mut self) {
        drop(self.quiesce());
    }
}
impl JackCapture {
    fn open() -> Result<Client> {
        Client::named(Api::load()?, c"shr-rec-capture")
    }
    fn inspect(client: &Client) -> Result<Inputs> {
        let mut sources = Vec::new();
        for name in client.port_names(2)? {
            let c = CString::new(name.as_str())?;
            let p = unsafe { (client.api.by_name)(client.client, c.as_ptr()) };
            ensure!(!p.is_null(), "source disappeared during discovery");
            sources.push(Source {
                name,
                uuid: unsafe { (client.api.uuid)(p) },
            });
        }
        Ok(Inputs {
            rate: unsafe { (client.api.rate)(client.client) },
            sources,
        })
    }
    fn attach_on(
        &mut self,
        client: Client,
        recorder: Recorder,
        sources: &[Source],
        rate: u32,
    ) -> std::result::Result<Arc<SharedCapture>, (Box<Recorder>, String)> {
        if sources.is_empty() || sources.len() > 64 {
            return Err((Box::new(recorder), "capture needs 1–64 sources".into()));
        }
        // Every fallible step is inside the closure, while Running owns the
        // producer and closes JACK before its callback storage can be released.
        let shared = Arc::new(SharedCapture::default());
        let mut r = CaptureRunning {
            client: Some(client),
            callback: Box::new(UnsafeCell::new(CaptureCallback {
                recorder: Some(recorder),
                shared: shared.clone(),
                ports: vec![],
                buffer: dummy_buffer,
                scratch: vec![0.; sources.len() * MAX_PERIOD],
                frame: 0,
            })),
            notifications: Box::new(CaptureNotifications {
                shared: shared.clone(),
                rate,
                edges: vec![],
            }),
            sources: vec![],
            ports: vec![],
        };
        let result = (|| -> Result<()> {
            ensure!(
                !sources.is_empty() && sources.len() <= 64,
                "capture needs 1–64 sources"
            );
            let client = r.client.as_mut().unwrap();
            let current = Self::inspect(client)?;
            ensure!(current.rate == rate, "JACK rate changed; refresh inputs");
            for (index, source) in sources.iter().enumerate() {
                ensure!(
                    current.sources.iter().filter(|s| *s == source).count() == 1,
                    "source missing/replaced: {}; refresh and inspect",
                    source.name
                );
                let name = CString::new(format!("in-{}", index + 1))?;
                let port = unsafe {
                    (client.api.register)(
                        client.client,
                        name.as_ptr(),
                        c"32 bit float mono audio".as_ptr(),
                        1,
                        0,
                    )
                };
                ensure!(!port.is_null(), "cannot register capture input");
                let source_name = CString::new(source.name.as_str())?;
                let source_port =
                    unsafe { (client.api.by_name)(client.client, source_name.as_ptr()) };
                ensure!(
                    !source_port.is_null()
                        && unsafe { (client.api.uuid)(source_port) } == source.uuid,
                    "source replaced before activation"
                );
                r.notifications.edges.push(unsafe {
                    (
                        (client.api.uuid_index)((client.api.uuid)(port)),
                        (client.api.uuid_index)(source.uuid),
                    )
                });
                r.sources.push((source_name, source.uuid));
                r.ports.push(port);
            }
            r.callback.get_mut().ports = r.ports.clone();
            r.callback.get_mut().buffer = client.api.buffer;
            let data = r.callback.get().cast();
            let n = ptr::from_ref(&*r.notifications).cast_mut().cast();
            ensure!(
                unsafe { (client.api.set_process)(client.client, capture_process, data) } == 0,
                "cannot install capture callback"
            );
            unsafe { (client.api.shutdown)(client.client, capture_shutdown, n) };
            ensure!(
                unsafe { (client.api.xrun)(client.client, capture_xrun, n) } == 0,
                "cannot install xrun callback"
            );
            ensure!(
                unsafe { (client.api.set_rate)(client.client, capture_rate, n) } == 0,
                "cannot install rate callback"
            );
            ensure!(
                unsafe { (client.api.set_connect)(client.client, capture_graph, n) } == 0,
                "cannot install graph callback"
            );
            ensure!(
                unsafe { (client.api.activate)(client.client) } == 0,
                "cannot activate capture"
            );
            client.active = true;
            for (port, (source, uuid)) in r.ports.iter().zip(&r.sources) {
                let p = unsafe { (client.api.by_name)(client.client, source.as_ptr()) };
                ensure!(
                    !p.is_null() && unsafe { (client.api.uuid)(p) } == *uuid,
                    "source replaced during attachment"
                );
                ensure!(
                    unsafe {
                        (client.api.connect)(
                            client.client,
                            source.as_ptr(),
                            (client.api.name)(*port),
                        )
                    } == 0,
                    "cannot connect selected capture source"
                );
                ensure!(
                    unsafe { (client.api.connected)(*port, source.as_ptr()) } != 0,
                    "capture edge missing"
                );
            }
            ensure!(
                unsafe { (client.api.rate)(client.client) } == rate,
                "JACK rate changed during attachment; refresh inputs"
            );
            ensure!(shared.failure() == 0, "JACK fault during startup");
            Ok(())
        })();
        if let Err(e) = result {
            shared.fault(ROUTE_LOST);
            return Err((Box::new(r.quiesce().unwrap()), format!("{e:#}")));
        }
        self.running = Some(r);
        Ok(shared)
    }
}
unsafe extern "C" fn dummy_buffer(_: *mut c_void, _: c_uint) -> *mut c_void {
    ptr::null_mut()
}
impl Device for JackCapture {
    fn inputs(&mut self) -> Result<Inputs> {
        Self::inspect(&Self::open()?)
    }
    fn attach(
        &mut self,
        recorder: Recorder,
        sources: &[Source],
        rate: u32,
    ) -> std::result::Result<Arc<SharedCapture>, (Box<Recorder>, String)> {
        if self.running.is_some() {
            return Err((Box::new(recorder), "capture already attached".into()));
        }
        match Self::open() {
            Ok(client) => self.attach_on(client, recorder, sources, rate),
            Err(e) => Err((Box::new(recorder), format!("{e:#}"))),
        }
    }
    fn poll(&mut self) {
        if let Some(r) = &self.running {
            let client = r.client.as_ref().unwrap();
            if unsafe { (client.api.rate)(client.client) } != r.notifications.rate {
                r.notifications.shared.fault(RATE_CHANGED);
            }
            for (port, (name, uuid)) in r.ports.iter().zip(&r.sources) {
                let source = unsafe { (client.api.by_name)(client.client, name.as_ptr()) };
                if source.is_null()
                    || unsafe { (client.api.uuid)(source) } != *uuid
                    || unsafe { (client.api.connected)(*port, name.as_ptr()) } == 0
                {
                    r.notifications.shared.fault(ROUTE_LOST);
                }
            }
        }
    }
    fn quiesce(&mut self) -> Option<Recorder> {
        self.running.take().and_then(|mut r| r.quiesce())
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{Stats, client};
    use super::*;
    use crate::recorder;
    use std::{
        alloc::{GlobalAlloc, Layout, System},
        cell::{Cell, RefCell},
        rc::Rc,
    };
    struct Guard;
    thread_local! {static ACTIVE:Cell<bool>=const{Cell::new(false)};static ALLOCS:Cell<usize>=const{Cell::new(0)};}
    unsafe impl GlobalAlloc for Guard {
        unsafe fn alloc(&self, l: Layout) -> *mut u8 {
            ACTIVE.with(|a| {
                if a.get() {
                    ALLOCS.with(|n| n.set(n.get() + 1))
                }
            });
            unsafe { System.alloc(l) }
        }
        unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
            ACTIVE.with(|a| {
                if a.get() {
                    ALLOCS.with(|n| n.set(n.get() + 1))
                }
            });
            unsafe { System.dealloc(p, l) }
        }
    }
    #[global_allocator]
    static ALLOCATOR: Guard = Guard;
    fn recorder(path: &std::path::Path) -> Recorder {
        Recorder::create(
            path,
            recorder::Config {
                sample_rate: 48000,
                channels: 2,
                max_block: 32,
                queue_blocks: 4,
                epoch: 1,
            },
        )
        .unwrap()
    }
    fn sources() -> Vec<Source> {
        vec![
            Source {
                name: "sink:left".into(),
                uuid: 1,
            },
            Source {
                name: "sink:right".into(),
                uuid: 2,
            },
        ]
    }
    #[test]
    fn capture_actual_adapter_readiness_connections_callback_faults_and_quiescence() {
        let base = std::env::temp_dir().join(format!("rec-capture-jack-{}", std::process::id()));
        std::fs::create_dir(&base).unwrap();
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(base.clone());
        let mut jack = JackCapture::default();
        for fail in 1..=8 {
            let path = base.join(format!("fail-{fail}"));
            let stats = Rc::new(RefCell::new(Stats::default()));
            let result = jack.attach_on(
                client(fail, stats.clone()),
                recorder(&path),
                &sources(),
                48000,
            );
            let (r, _) = result.err().expect("failure stage");
            assert_eq!(r.observer().snapshot().accepted_frames, 0);
            assert_eq!((*r).finish().unwrap().status, "incomplete");
            assert_eq!(stats.borrow().closed, 1);
            assert_eq!(stats.borrow().flags, 3);
        }
        for fault in 0..7 {
            let path = base.join(format!("run-{fault}"));
            let stats = Rc::new(RefCell::new(Stats::default()));
            let r = recorder(&path);
            let observer = r.observer();
            let shared = jack
                .attach_on(client(0, stats.clone()), r, &sources(), 48000)
                .ok()
                .unwrap();
            assert_eq!(stats.borrow().connected, 2);
            assert_eq!(observer.snapshot().accepted_frames, 0); // activation callback is inert
            shared.enable();
            let running = jack.running.as_ref().unwrap();
            let data = running.callback.get().cast();
            let n = ptr::from_ref(&*running.notifications).cast_mut().cast();
            let edge = running.notifications.edges[0];
            ACTIVE.with(|a| a.set(true));
            assert_eq!(unsafe { capture_process(32, data) }, 0);
            unsafe { capture_graph(123, 456, 0, n) };
            assert_eq!(shared.failure(), 0);
            match fault {
                0 => {}
                1 => unsafe { capture_graph(edge.0, edge.1, 0, n) },
                2 => unsafe { capture_shutdown(n) },
                3 => {
                    unsafe { capture_rate(44100, n) };
                }
                4 => {
                    unsafe { capture_xrun(n) };
                }
                5 => {
                    assert_eq!(unsafe { capture_process(8193, data) }, 1);
                }
                6 => {
                    // Deterministic refused-push path: this test writer permits
                    // 32-frame periods. Raw queue overflow has a stopped-consumer
                    // regression in recorder.rs; never race disk throughput here.
                    unsafe { capture_process(64, data) };
                }
                _ => unreachable!(),
            }
            if fault != 0 {
                unsafe { capture_process(32, data) };
            }
            ACTIVE.with(|a| a.set(false));
            assert_eq!(ALLOCS.with(Cell::get), 0);
            assert_eq!(shared.peak(0), 0.25);
            assert_eq!(shared.peak(1), 0.5);
            let r = jack.quiesce().unwrap();
            assert_eq!(stats.borrow().closed, 1);
            assert_eq!(stats.borrow().deactivated, 1);
            let report = r.finish().unwrap();
            assert_eq!(
                report.status,
                if fault == 0 { "complete" } else { "incomplete" }
            );
            let take = Take::open(&path).unwrap();
            assert_eq!(take.stems.len(), 2);
            if fault != 0 {
                assert!(take.warning.is_some());
            } else {
                assert_eq!(report.frames, 32);
            }
        }
        for (rate, uuid) in [(44100, 1), (48000, 99)] {
            let path = base.join(format!("stale-{rate}-{uuid}"));
            let stats = Rc::new(RefCell::new(Stats::default()));
            let mut selected = sources();
            selected[0].uuid = uuid;
            let (r, _) = jack
                .attach_on(client(0, stats.clone()), recorder(&path), &selected, rate)
                .err()
                .unwrap();
            assert_eq!(stats.borrow().connected, 0);
            assert_eq!(stats.borrow().closed, 1);
            assert_eq!((*r).finish().unwrap().status, "incomplete");
        }
        std::fs::remove_dir_all(base).unwrap();
    }
}
