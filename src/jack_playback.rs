//! Minimal dynamic JACK boundary following SHR's no-start, exact-connection
//! approach. No link-time JACK dependency and no device access until requested.
use crate::{
    playback::{self, Prefetch, Render, Routing, Shared},
    take::Take,
};
use anyhow::{Result, ensure};
use libc::{c_char, c_int, c_uint, c_ulong, c_void};
use std::{
    cell::UnsafeCell,
    ffi::{CStr, CString},
    ptr,
    sync::Arc,
};

type Process = unsafe extern "C" fn(c_uint, *mut c_void) -> c_int;
type PortConnect = unsafe extern "C" fn(c_uint, c_uint, c_int, *mut c_void);
type Notify = unsafe extern "C" fn(*mut c_void);
type GetBuffer = unsafe extern "C" fn(*mut c_void, c_uint) -> *mut c_void;
struct Api {
    handle: *mut c_void,
    open: unsafe extern "C" fn(*const c_char, c_uint, *mut c_uint) -> *mut c_void,
    close: unsafe extern "C" fn(*mut c_void) -> c_int,
    rate: unsafe extern "C" fn(*mut c_void) -> c_uint,
    register: unsafe extern "C" fn(
        *mut c_void,
        *const c_char,
        *const c_char,
        c_ulong,
        c_ulong,
    ) -> *mut c_void,
    name: unsafe extern "C" fn(*mut c_void) -> *const c_char,
    set_process: unsafe extern "C" fn(*mut c_void, Process, *mut c_void) -> c_int,
    set_connect: unsafe extern "C" fn(*mut c_void, PortConnect, *mut c_void) -> c_int,
    by_name: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    uuid: unsafe extern "C" fn(*const c_void) -> u64,
    uuid_index: unsafe extern "C" fn(u64) -> c_uint,
    set_rate: unsafe extern "C" fn(*mut c_void, Process, *mut c_void) -> c_int,
    shutdown: unsafe extern "C" fn(*mut c_void, Notify, *mut c_void),
    xrun: unsafe extern "C" fn(*mut c_void, ProcessXrun, *mut c_void) -> c_int,
    activate: unsafe extern "C" fn(*mut c_void) -> c_int,
    deactivate: unsafe extern "C" fn(*mut c_void) -> c_int,
    connect: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> c_int,
    ports: unsafe extern "C" fn(
        *mut c_void,
        *const c_char,
        *const c_char,
        c_ulong,
    ) -> *mut *const c_char,
    free: unsafe extern "C" fn(*mut c_void),
    buffer: GetBuffer,
    connected: unsafe extern "C" fn(*const c_void, *const c_char) -> c_int,
}
type ProcessXrun = unsafe extern "C" fn(*mut c_void) -> c_int;
impl Api {
    fn load() -> Result<Self> {
        let handle =
            unsafe { libc::dlopen(c"libjack.so.0".as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        ensure!(!handle.is_null(), "JACK library unavailable");
        macro_rules! symbol {
            ($name:literal,$ty:ty) => {{
                let p = unsafe { libc::dlsym(handle, concat!($name, "\0").as_ptr().cast()) };
                if p.is_null() {
                    unsafe { libc::dlclose(handle) };
                    anyhow::bail!("missing JACK symbol {}", $name);
                }
                unsafe { std::mem::transmute::<*mut c_void, $ty>(p) }
            }};
        }
        Ok(Self {
            handle,
            open: symbol!(
                "jack_client_open",
                unsafe extern "C" fn(*const c_char, c_uint, *mut c_uint) -> *mut c_void
            ),
            close: symbol!(
                "jack_client_close",
                unsafe extern "C" fn(*mut c_void) -> c_int
            ),
            rate: symbol!(
                "jack_get_sample_rate",
                unsafe extern "C" fn(*mut c_void) -> c_uint
            ),
            register: symbol!(
                "jack_port_register",
                unsafe extern "C" fn(
                    *mut c_void,
                    *const c_char,
                    *const c_char,
                    c_ulong,
                    c_ulong,
                ) -> *mut c_void
            ),
            name: symbol!(
                "jack_port_name",
                unsafe extern "C" fn(*mut c_void) -> *const c_char
            ),
            set_process: symbol!(
                "jack_set_process_callback",
                unsafe extern "C" fn(*mut c_void, Process, *mut c_void) -> c_int
            ),
            set_connect: symbol!(
                "jack_set_port_connect_callback",
                unsafe extern "C" fn(*mut c_void, PortConnect, *mut c_void) -> c_int
            ),
            by_name: symbol!(
                "jack_port_by_name",
                unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void
            ),
            uuid: symbol!("jack_port_uuid", unsafe extern "C" fn(*const c_void) -> u64),
            uuid_index: symbol!("jack_uuid_to_index", unsafe extern "C" fn(u64) -> c_uint),
            set_rate: symbol!(
                "jack_set_sample_rate_callback",
                unsafe extern "C" fn(*mut c_void, Process, *mut c_void) -> c_int
            ),
            shutdown: symbol!(
                "jack_on_shutdown",
                unsafe extern "C" fn(*mut c_void, Notify, *mut c_void)
            ),
            xrun: symbol!(
                "jack_set_xrun_callback",
                unsafe extern "C" fn(*mut c_void, ProcessXrun, *mut c_void) -> c_int
            ),
            activate: symbol!("jack_activate", unsafe extern "C" fn(*mut c_void) -> c_int),
            deactivate: symbol!(
                "jack_deactivate",
                unsafe extern "C" fn(*mut c_void) -> c_int
            ),
            connect: symbol!(
                "jack_connect",
                unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> c_int
            ),
            ports: symbol!(
                "jack_get_ports",
                unsafe extern "C" fn(
                    *mut c_void,
                    *const c_char,
                    *const c_char,
                    c_ulong,
                ) -> *mut *const c_char
            ),
            free: symbol!("jack_free", unsafe extern "C" fn(*mut c_void)),
            buffer: symbol!("jack_port_get_buffer", GetBuffer),
            connected: symbol!(
                "jack_port_connected_to",
                unsafe extern "C" fn(*const c_void, *const c_char) -> c_int
            ),
        })
    }
}
impl Drop for Api {
    fn drop(&mut self) {
        unsafe {
            if !self.handle.is_null() {
                libc::dlclose(self.handle);
            }
        }
    }
}
struct Client {
    api: Api,
    client: *mut c_void,
    active: bool,
}
impl Client {
    fn open() -> Result<Self> {
        Self::from_api(Api::load()?)
    }
    fn from_api(api: Api) -> Result<Self> {
        let mut status = 0;
        // JackNoStartServer | JackUseExactName. Never reconfigure/start JACK.
        let client = unsafe { (api.open)(c"shr-rec-playback".as_ptr(), 3, &mut status) };
        ensure!(
            !client.is_null(),
            "cannot attach to existing JACK server (status {status}); client name must be free"
        );
        Ok(Self {
            api,
            client,
            active: false,
        })
    }
    fn outputs(&self) -> Result<Vec<String>> {
        let list = unsafe {
            (self.api.ports)(
                self.client,
                ptr::null(),
                c"32 bit float mono audio".as_ptr(),
                1,
            )
        };
        if list.is_null() {
            return Ok(Vec::new());
        }
        let mut names = Vec::new();
        // JACK owns the null-terminated snapshot until jack_free.
        unsafe {
            for i in 0..4096 {
                let p = *list.add(i);
                if p.is_null() {
                    break;
                }
                names.push(CStr::from_ptr(p).to_string_lossy().into_owned());
            }
            (self.api.free)(list.cast());
        }
        ensure!(names.len() < 4096, "too many JACK output destinations");
        names.sort();
        Ok(names)
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        unsafe {
            if self.active {
                (self.api.deactivate)(self.client);
            }
            (self.api.close)(self.client);
        }
    }
}

pub fn outputs() -> Result<Vec<String>> {
    Client::open()?.outputs()
}
struct Callback {
    render: Render,
    ports: Vec<*mut c_void>,
    buffer: GetBuffer,
}
unsafe extern "C" fn process(frames: c_uint, data: *mut c_void) -> c_int {
    // SAFETY: boxed data stays live until client deactivation/close. JACK gives
    // this callback exclusive ownership of Render and distinct output buffers.
    let callback = unsafe { &mut *data.cast::<Callback>() };
    if frames == 0 || frames as usize > playback::MAX_PERIOD {
        callback.render.shared.fault(playback::State::InvalidPeriod);
        // JACK removes a nonzero-returning client from its process graph. Do
        // not traverse an unsupported, potentially unbounded buffer length.
        return 1;
    }
    let mut outputs: [&mut [f32]; 64] = std::array::from_fn(|_| &mut [][..]);
    for (index, port) in callback.ports.iter().enumerate() {
        let buffer = unsafe { (callback.buffer)(*port, frames) }.cast::<f32>();
        if buffer.is_null() {
            callback.render.shared.fault(playback::State::RouteLost);
            continue;
        }
        outputs[index] = unsafe { std::slice::from_raw_parts_mut(buffer, frames as usize) };
    }
    callback
        .render
        .process(&mut outputs[..callback.ports.len()]);
    0
}
struct Notifications {
    shared: Arc<Shared>,
    rate: u32,
    edges: Vec<(u32, u32)>,
}
unsafe extern "C" fn graph_changed(a: c_uint, b: c_uint, connected: c_int, data: *mut c_void) {
    let state = unsafe { &*data.cast::<Notifications>() };
    if connected == 0
        && state
            .edges
            .iter()
            .any(|&(x, y)| (x == a && y == b) || (x == b && y == a))
    {
        state.shared.fault(playback::State::RouteLost);
    }
}
unsafe extern "C" fn shutdown(data: *mut c_void) {
    unsafe { &*data.cast::<Notifications>() }
        .shared
        .device_lost();
}
unsafe extern "C" fn sample_rate(rate: c_uint, data: *mut c_void) -> c_int {
    let state = unsafe { &*data.cast::<Notifications>() };
    if rate != state.rate {
        state.shared.fault(playback::State::RateMismatch);
    }
    0
}
unsafe extern "C" fn xrun(data: *mut c_void) -> c_int {
    unsafe { &*data.cast::<Notifications>() }
        .shared
        .fault(playback::State::Xrun);
    0
}

pub trait Device {
    fn outputs(&mut self) -> Result<Vec<String>>;
    fn start(
        &mut self,
        take: Take,
        routing: &Routing,
        allow_damaged: bool,
        start: u64,
    ) -> Result<Arc<Shared>>;
    fn stop(&mut self);
    fn poll(&mut self) {}
}
#[derive(Default)]
pub struct Jack {
    running: Option<Running>,
}
struct Running {
    client: Option<Client>,
    _callback: Box<UnsafeCell<Callback>>,
    _prefetch: Prefetch,
    shared: Arc<Shared>,
    destinations: Vec<CString>,
    ports: Vec<*mut c_void>,
    _notifications: Box<Notifications>,
}
impl Drop for Running {
    fn drop(&mut self) {
        self.shared.stop();
        drop(self.client.take()); /* callback/worker drop only after JACK closes */
    }
}
impl Device for Jack {
    fn outputs(&mut self) -> Result<Vec<String>> {
        outputs()
    }
    fn start(
        &mut self,
        take: Take,
        routing: &Routing,
        allow_damaged: bool,
        start: u64,
    ) -> Result<Arc<Shared>> {
        self.stop();
        self.start_on(Client::open()?, take, routing, allow_damaged, start)
    }
    fn stop(&mut self) {
        drop(self.running.take());
    }
    fn poll(&mut self) {
        if let Some(r) = &self.running {
            if !matches!(
                r.shared.state(),
                playback::State::Playing | playback::State::Paused
            ) {
                return;
            }
            let client = r.client.as_ref().expect("live client");
            for (port, dest) in r.ports.iter().zip(&r.destinations) {
                if unsafe { (client.api.connected)(*port, dest.as_ptr()) } == 0 {
                    r.shared.fault(playback::State::RouteLost);
                }
            }
        }
    }
}

impl Jack {
    fn start_on(
        &mut self,
        client: Client,
        take: Take,
        routing: &Routing,
        allow_damaged: bool,
        start: u64,
    ) -> Result<Arc<Shared>> {
        self.stop();
        let available = client.outputs()?;
        let destinations = routing.ports();
        for name in &destinations {
            ensure!(
                available.iter().filter(|p| *p == name).count() == 1,
                "missing/ambiguous exact output: {name}"
            );
        }
        let rate = unsafe { (client.api.rate)(client.client) };
        let (render, prefetch) = playback::prepare(take, routing, rate, allow_damaged, start)?;
        let shared = render.shared.clone();
        let mut ports = Vec::new();
        for index in 0..destinations.len() {
            let name = CString::new(format!("out-{}", index + 1))?;
            let port = unsafe {
                (client.api.register)(
                    client.client,
                    name.as_ptr(),
                    c"32 bit float mono audio".as_ptr(),
                    2,
                    0,
                )
            };
            ensure!(!port.is_null(), "cannot register JACK output");
            ports.push(port);
        }
        let callback = Box::new(UnsafeCell::new(Callback {
            render,
            ports: ports.clone(),
            buffer: client.api.buffer,
        }));
        let c_destinations = destinations
            .iter()
            .map(|s| CString::new(s.as_str()))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut edges = Vec::with_capacity(ports.len());
        for (port, destination) in ports.iter().zip(&c_destinations) {
            let target = unsafe { (client.api.by_name)(client.client, destination.as_ptr()) };
            ensure!(
                !target.is_null(),
                "exact output disappeared before activation"
            );
            edges.push(unsafe {
                (
                    (client.api.uuid_index)((client.api.uuid)(*port)),
                    (client.api.uuid_index)((client.api.uuid)(target)),
                )
            });
        }
        let notifications = Box::new(Notifications {
            shared: shared.clone(),
            rate,
            edges,
        });
        // Own all callback lifetimes BEFORE registering any callbacks. Every
        // failure below closes JACK before releasing callback data or Shared.
        let mut running = Running {
            client: Some(client),
            _callback: callback,
            _prefetch: prefetch,
            shared: shared.clone(),
            destinations: c_destinations,
            ports,
            _notifications: notifications,
        };
        let data = running._callback.get().cast();
        let shared_data = ptr::from_ref(&*running._notifications).cast_mut().cast();
        let client = running.client.as_mut().expect("live client");
        ensure!(
            unsafe { (client.api.set_process)(client.client, process, data) } == 0,
            "cannot install JACK callback"
        );
        unsafe {
            (client.api.shutdown)(client.client, shutdown, shared_data);
        }
        ensure!(
            unsafe { (client.api.xrun)(client.client, xrun, shared_data) } == 0,
            "cannot install JACK xrun callback"
        );
        ensure!(
            unsafe { (client.api.set_rate)(client.client, sample_rate, shared_data) } == 0,
            "cannot install JACK sample-rate callback"
        );
        ensure!(
            unsafe { (client.api.set_connect)(client.client, graph_changed, shared_data) } == 0,
            "cannot install exact-route callback"
        );
        ensure!(
            unsafe { (client.api.activate)(client.client) } == 0,
            "cannot activate JACK playback"
        );
        client.active = true;
        for (port, destination) in running.ports.iter().zip(&running.destinations) {
            ensure!(
                unsafe {
                    (client.api.connect)(
                        client.client,
                        (client.api.name)(*port),
                        destination.as_ptr(),
                    )
                } == 0,
                "cannot connect exact JACK output"
            );
        }
        ensure!(
            shared.state() == playback::State::Paused,
            "JACK changed during playback startup"
        );
        self.running = Some(running);
        Ok(shared)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};
    #[derive(Default)]
    struct Stats {
        flags: u32,
        closed: usize,
        deactivated: usize,
        connected: usize,
    }
    thread_local! {static SETUP: RefCell<Option<(u8,Rc<RefCell<Stats>>)>>=const{RefCell::new(None)};}
    struct FakePort {
        name: CString,
        samples: [f32; 8192],
        connected: bool,
    }
    struct FakeClient {
        fail: u8,
        stats: Rc<RefCell<Stats>>,
        ports: Vec<*mut FakePort>,
        process: Option<Process>,
        process_data: *mut c_void,
        shutdown: Option<Notify>,
        notify_data: *mut c_void,
    }
    unsafe extern "C" fn open(_: *const c_char, flags: c_uint, _: *mut c_uint) -> *mut c_void {
        let (fail, stats) = SETUP.with(|s| s.borrow_mut().take().expect("fake setup"));
        stats.borrow_mut().flags = flags;
        Box::into_raw(Box::new(FakeClient {
            fail,
            stats,
            ports: vec![],
            process: None,
            process_data: ptr::null_mut(),
            shutdown: None,
            notify_data: ptr::null_mut(),
        }))
        .cast()
    }
    unsafe extern "C" fn close(client: *mut c_void) -> c_int {
        let fake = unsafe { Box::from_raw(client.cast::<FakeClient>()) };
        // Exercise the installed callbacks during close, including failed
        // startup: their boxed data and immutable notifications must still live.
        if let Some(process) = fake.process {
            unsafe {
                process(32, fake.process_data);
            }
        }
        if let Some(shutdown) = fake.shutdown {
            unsafe {
                shutdown(fake.notify_data);
            }
        }
        for port in &fake.ports {
            unsafe {
                drop(Box::from_raw(*port));
            }
        }
        fake.stats.borrow_mut().closed += 1;
        0
    }
    unsafe extern "C" fn rate(_: *mut c_void) -> c_uint {
        48000
    }
    unsafe extern "C" fn register(
        client: *mut c_void,
        name: *const c_char,
        _: *const c_char,
        _: c_ulong,
        _: c_ulong,
    ) -> *mut c_void {
        let fake = unsafe { &mut *client.cast::<FakeClient>() };
        if fake.fail == 1 {
            return ptr::null_mut();
        }
        let port = Box::into_raw(Box::new(FakePort {
            name: unsafe { CStr::from_ptr(name) }.to_owned(),
            samples: [99.; 8192],
            connected: false,
        }));
        fake.ports.push(port);
        port.cast()
    }
    unsafe extern "C" fn name(port: *mut c_void) -> *const c_char {
        unsafe { &*port.cast::<FakePort>() }.name.as_ptr()
    }
    unsafe extern "C" fn buffer(port: *mut c_void, _: c_uint) -> *mut c_void {
        unsafe { &mut *port.cast::<FakePort>() }
            .samples
            .as_mut_ptr()
            .cast()
    }
    unsafe extern "C" fn set_process(
        client: *mut c_void,
        callback: Process,
        data: *mut c_void,
    ) -> c_int {
        let f = unsafe { &mut *client.cast::<FakeClient>() };
        if f.fail == 2 {
            return -1;
        }
        f.process = Some(callback);
        f.process_data = data;
        0
    }
    unsafe extern "C" fn on_shutdown(client: *mut c_void, callback: Notify, data: *mut c_void) {
        let f = unsafe { &mut *client.cast::<FakeClient>() };
        f.shutdown = Some(callback);
        f.notify_data = data;
    }
    unsafe extern "C" fn set_xrun(client: *mut c_void, _: ProcessXrun, _: *mut c_void) -> c_int {
        if unsafe { &*client.cast::<FakeClient>() }.fail == 3 {
            -1
        } else {
            0
        }
    }
    unsafe extern "C" fn set_connect(client: *mut c_void, _: PortConnect, _: *mut c_void) -> c_int {
        if unsafe { &*client.cast::<FakeClient>() }.fail == 7 {
            -1
        } else {
            0
        }
    }
    unsafe extern "C" fn by_name(_: *mut c_void, name: *const c_char) -> *mut c_void {
        if unsafe { CStr::from_ptr(name) } == c"sink:left" {
            ptr::without_provenance_mut(1)
        } else {
            ptr::without_provenance_mut(2)
        }
    }
    unsafe extern "C" fn uuid(port: *const c_void) -> u64 {
        port as usize as u64
    }
    unsafe extern "C" fn uuid_index(uuid: u64) -> c_uint {
        uuid as u32
    }
    unsafe extern "C" fn set_rate(client: *mut c_void, _: Process, _: *mut c_void) -> c_int {
        if unsafe { &*client.cast::<FakeClient>() }.fail == 4 {
            -1
        } else {
            0
        }
    }
    unsafe extern "C" fn activate(client: *mut c_void) -> c_int {
        let f = unsafe { &*client.cast::<FakeClient>() };
        if f.fail == 5 {
            -1
        } else {
            if let Some(process) = f.process {
                unsafe {
                    process(32, f.process_data);
                }
            }
            0
        }
    }
    unsafe extern "C" fn deactivate(client: *mut c_void) -> c_int {
        unsafe { &*client.cast::<FakeClient>() }
            .stats
            .borrow_mut()
            .deactivated += 1;
        0
    }
    unsafe extern "C" fn connect(
        client: *mut c_void,
        source: *const c_char,
        _: *const c_char,
    ) -> c_int {
        let f = unsafe { &mut *client.cast::<FakeClient>() };
        let mut stats = f.stats.borrow_mut();
        stats.connected += 1;
        if f.fail == 6 && stats.connected == 2 {
            return -1;
        }
        for port in &f.ports {
            let p = unsafe { &mut **port };
            if p.name.as_c_str() == unsafe { CStr::from_ptr(source) } {
                p.connected = true;
            }
        }
        0
    }
    unsafe extern "C" fn ports(
        _: *mut c_void,
        _: *const c_char,
        _: *const c_char,
        _: c_ulong,
    ) -> *mut *const c_char {
        let list = unsafe { libc::malloc(3 * std::mem::size_of::<*const c_char>()) }
            .cast::<*const c_char>();
        unsafe {
            *list = c"sink:left".as_ptr();
            *list.add(1) = c"sink:right".as_ptr();
            *list.add(2) = ptr::null();
        }
        list
    }
    unsafe extern "C" fn free(p: *mut c_void) {
        unsafe {
            libc::free(p);
        }
    }
    unsafe extern "C" fn connected(port: *const c_void, _: *const c_char) -> c_int {
        i32::from(unsafe { &*port.cast::<FakePort>() }.connected)
    }
    fn client(fail: u8, stats: Rc<RefCell<Stats>>) -> Client {
        SETUP.with(|s| *s.borrow_mut() = Some((fail, stats)));
        Client::from_api(Api {
            handle: ptr::null_mut(),
            open,
            close,
            rate,
            register,
            name,
            set_process,
            shutdown: on_shutdown,
            xrun: set_xrun,
            set_rate,
            set_connect,
            by_name,
            uuid,
            uuid_index,
            activate,
            deactivate,
            connect,
            ports,
            free,
            buffer,
            connected,
        })
        .unwrap()
    }
    #[test]
    fn actual_adapter_with_fake_jack_covers_startup_failures_exact_routes_and_quiescence() {
        use crate::recorder::{Config, Recorder};
        let path = std::env::temp_dir().join(format!("rec-jack-fake-{}", std::process::id()));
        let mut recorder = Recorder::create(
            &path,
            Config {
                sample_rate: 48000,
                channels: 2,
                max_block: 32,
                queue_blocks: 8,
                epoch: 1,
            },
        )
        .unwrap();
        assert_eq!(recorder.push(0, &[0.25, -0.5].repeat(32), 32), 0);
        recorder.finish().unwrap();
        let routing = Routing::Exact {
            outputs: vec!["sink:left".into(), "sink:right".into()],
        };
        let mut jack = Jack::default();
        for fail in 1..=7 {
            let stats = Rc::new(RefCell::new(Stats::default()));
            let c = client(fail, stats.clone());
            assert!(
                jack.start_on(c, Take::open(&path).unwrap(), &routing, false, 0)
                    .is_err(),
                "failure stage {fail}"
            );
            assert!(jack.running.is_none());
            let s = stats.borrow();
            assert_eq!(s.flags, 3);
            assert_eq!(s.closed, 1);
            assert_eq!(s.deactivated, usize::from(fail == 6));
        }
        let stats = Rc::new(RefCell::new(Stats::default()));
        let c = client(0, stats.clone());
        let shared = jack
            .start_on(c, Take::open(&path).unwrap(), &routing, false, 0)
            .unwrap();
        assert_eq!(stats.borrow().connected, 2);
        assert_eq!(shared.position(), 0);
        shared.play().unwrap();
        let r = jack.running.as_ref().unwrap();
        let c = unsafe { &*r.client.as_ref().unwrap().client.cast::<FakeClient>() };
        unsafe {
            c.process.unwrap()(32, c.process_data);
        }
        assert_eq!(shared.position(), 32);
        assert_eq!(shared.state(), playback::State::Eof);
        let left = unsafe { &*c.ports[0] };
        let right = unsafe { &*c.ports[1] };
        assert_eq!(left.samples[0], 0.25);
        assert_eq!(right.samples[31], -0.5);
        jack.stop();
        assert_eq!(stats.borrow().deactivated, 1);
        assert_eq!(stats.borrow().closed, 1);
        for fault in 0..5 {
            let stats = Rc::new(RefCell::new(Stats::default()));
            let shared = jack
                .start_on(
                    client(0, stats.clone()),
                    Take::open(&path).unwrap(),
                    &routing,
                    false,
                    0,
                )
                .unwrap();
            let r = jack.running.as_ref().unwrap();
            let data = ptr::from_ref(&*r._notifications).cast_mut().cast();
            let edge = r._notifications.edges[0];
            let c = unsafe { &*r.client.as_ref().unwrap().client.cast::<FakeClient>() };
            shared.play().unwrap();
            let expected = match fault {
                0 => {
                    unsafe {
                        sample_rate(48000, data);
                    }
                    assert_eq!(shared.state(), playback::State::Playing);
                    unsafe {
                        sample_rate(44100, data);
                    }
                    playback::State::RateMismatch
                }
                1 => {
                    unsafe {
                        xrun(data);
                    }
                    playback::State::Xrun
                }
                2 => {
                    unsafe {
                        shutdown(data);
                    }
                    playback::State::DeviceLost
                }
                3 => {
                    unsafe {
                        graph_changed(123, 456, 0, data);
                    }
                    assert_eq!(shared.state(), playback::State::Playing);
                    unsafe {
                        graph_changed(edge.1, edge.0, 0, data);
                    }
                    playback::State::RouteLost
                }
                _ => {
                    assert_eq!(unsafe { c.process.unwrap()(8193, c.process_data) }, 1);
                    playback::State::InvalidPeriod
                }
            };
            assert_eq!(shared.state(), expected);
            unsafe {
                c.process.unwrap()(32, c.process_data);
            }
            assert_eq!(shared.position(), 0);
            for port in &c.ports {
                assert!(unsafe { &**port }.samples[..32].iter().all(|v| *v == 0.));
            }
            jack.stop();
            assert_eq!(stats.borrow().closed, 1);
        }
        // Missing exact output fails before activation; no first/adjacent fallback.
        let stats = Rc::new(RefCell::new(Stats::default()));
        assert!(
            jack.start_on(
                client(0, stats.clone()),
                Take::open(&path).unwrap(),
                &Routing::Exact {
                    outputs: vec!["missing".into(), "sink:right".into()]
                },
                false,
                0
            )
            .is_err()
        );
        assert_eq!(stats.borrow().closed, 1);
        assert_eq!(stats.borrow().connected, 0);
        std::fs::remove_dir_all(path).unwrap();
    }
}
