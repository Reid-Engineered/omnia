//! Dynamic FFI loader for `libngspice` (`ngspice.dll` / `libngspice.so`)
//! matching `src/include/ngspice/sharedspice.h` from ngspice 47.
//!
//! Safety & concurrency invariants:
//! - `libngspice` keeps process-global C state, so `ACTIVE_INSTANCE` enforces a single live
//!   `NgspiceLib` per process (`InstanceGuard` refuses any second load until cleanly dropped).
//! - `NgspiceLib` is `!Sync` and requires `&mut self` for all FFI calls, preventing concurrent
//!   FFI entry across threads.
//! - `Drop` stops and waits for any active background solver thread (`bg_halt` + wait) and
//!   detaches callbacks. Because ngspice invokes `cb_bg_thread_running` before its solver
//!   thread returns (and spawns detached pthreads on Unix), `Drop` unloads `libloading::Library`,
//!   frees `CallbackState`, and releases `ACTIVE_INSTANCE` only when no background solver thread
//!   was ever started (`!self.bg_ever_started && !self.is_running()`); otherwise it pins the
//!   library, callback state, and singleton guard for the process lifetime so no background
//!   thread can execute unloaded code or access freed callback state.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::marker::PhantomData;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use libloading::Library;

static ACTIVE_INSTANCE: AtomicBool = AtomicBool::new(false);

/// RAII token enforcing at most one active `libngspice` instance per process.
#[derive(Debug)]
pub struct InstanceGuard {
    _private: (),
}

impl InstanceGuard {
    pub fn acquire() -> Result<Self, String> {
        ACTIVE_INSTANCE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self { _private: () })
            .map_err(|_| {
                "libngspice is already loaded in this process (exclusive singleton guard)".into()
            })
    }
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        ACTIVE_INSTANCE.store(false, Ordering::Release);
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct NgComplex {
    pub cx_real: f64,
    pub cx_imag: f64,
}

#[repr(C)]
#[derive(Debug)]
pub struct VectorInfo {
    pub v_name: *mut c_char,
    pub v_type: c_int,
    pub v_flags: i16,
    pub v_realdata: *mut f64,
    pub v_compdata: *mut NgComplex,
    pub v_length: c_int,
}

#[repr(C)]
#[derive(Debug)]
pub struct VecValues {
    pub name: *mut c_char,
    pub creal: f64,
    pub cimag: f64,
    pub is_scale: bool,
    pub is_complex: bool,
}

#[repr(C)]
#[derive(Debug)]
pub struct VecValuesAll {
    pub veccount: c_int,
    pub vecindex: c_int,
    pub vecsa: *mut *mut VecValues,
}

#[repr(C)]
#[derive(Debug)]
pub struct VecInfo {
    pub number: c_int,
    pub vecname: *mut c_char,
    pub is_real: bool,
    pub pdvec: *mut c_void,
    pub pdvecscale: *mut c_void,
}

#[repr(C)]
#[derive(Debug)]
pub struct VecInfoAll {
    pub name: *mut c_char,
    pub title: *mut c_char,
    pub date: *mut c_char,
    pub r#type: *mut c_char,
    pub veccount: c_int,
    pub vecs: *mut *mut VecInfo,
}

type SendCharFn = unsafe extern "C" fn(*mut c_char, c_int, *mut c_void) -> c_int;
type SendStatFn = unsafe extern "C" fn(*mut c_char, c_int, *mut c_void) -> c_int;
type ControlledExitFn = unsafe extern "C" fn(c_int, bool, bool, c_int, *mut c_void) -> c_int;
type SendDataFn = unsafe extern "C" fn(*mut VecValuesAll, c_int, c_int, *mut c_void) -> c_int;
type SendInitDataFn = unsafe extern "C" fn(*mut VecInfoAll, c_int, *mut c_void) -> c_int;
type BgThreadRunningFn = unsafe extern "C" fn(bool, c_int, *mut c_void) -> c_int;

type NgSpiceInitFn = unsafe extern "C" fn(
    Option<SendCharFn>,
    Option<SendStatFn>,
    Option<ControlledExitFn>,
    Option<SendDataFn>,
    Option<SendInitDataFn>,
    Option<BgThreadRunningFn>,
    *mut c_void,
) -> c_int;
type NgSpiceCommandFn = unsafe extern "C" fn(*mut c_char) -> c_int;
type NgSpiceCircFn = unsafe extern "C" fn(*mut *mut c_char) -> c_int;
type NgSpiceRunningFn = unsafe extern "C" fn() -> bool;
type NgGetVecInfoFn = unsafe extern "C" fn(*mut c_char) -> *mut VectorInfo;

/// Callback sink invoked on every accepted transient point inside `SendData`.
pub trait SampleSink: Send {
    fn on_point(&mut self, t: f64, values: &[f64]);
}

/// Shared wakeup trigger allowing external control threads (e.g., stdin `Cancel` reader) to
/// wake the single `NgspiceLib` owner thread immediately without touching `libngspice` FFI.
#[derive(Clone)]
pub struct BgWakeup {
    inner: Arc<(Mutex<WakeState>, Condvar)>,
}

struct WakeState {
    bg_finished: bool,
    cancel_requested: bool,
}

impl BgWakeup {
    fn new() -> Self {
        Self {
            inner: Arc::new((
                Mutex::new(WakeState {
                    bg_finished: true,
                    cancel_requested: false,
                }),
                Condvar::new(),
            )),
        }
    }

    fn reset_for_run(&self) {
        if let Ok(mut g) = self.inner.0.lock() {
            g.bg_finished = false;
            g.cancel_requested = false;
        }
    }

    fn mark_bg_finished(&self) {
        if let Ok(mut g) = self.inner.0.lock() {
            g.bg_finished = true;
            self.inner.1.notify_all();
        }
    }

    /// Signal the `NgspiceLib` owner thread that a cancel was requested.
    pub fn request_cancel(&self) {
        if let Ok(mut g) = self.inner.0.lock() {
            g.cancel_requested = true;
            self.inner.1.notify_all();
        }
    }
}

/// Shared state bridged via `userData` pointer to `libngspice` C callbacks.
struct CallbackState {
    detached: AtomicBool,
    bg_ever_started: AtomicBool,
    wakeup: BgWakeup,
    inner: Mutex<CallbackInner>,
}

struct CallbackInner {
    requested_probes: Vec<String>,
    scale_idx: Option<usize>,
    probe_indices: Vec<usize>,
    missing_probe: Option<String>,
    scratch_vals: Vec<f64>,
    sink: Option<Box<dyn SampleSink>>,
    total_points: u64,
    first_sample_at: Option<Instant>,
    last_error: Option<String>,
    stdout_tail: Vec<String>,
}

impl CallbackInner {
    fn new() -> Self {
        Self {
            requested_probes: Vec::new(),
            scale_idx: None,
            probe_indices: Vec::new(),
            missing_probe: None,
            scratch_vals: Vec::new(),
            sink: None,
            total_points: 0,
            first_sample_at: None,
            last_error: None,
            stdout_tail: Vec::new(),
        }
    }
}

fn normalize_probe(name: &str) -> String {
    let trimmed = name.trim().to_ascii_lowercase();
    if let Some(inner) = trimmed.strip_prefix("v(").and_then(|s| s.strip_suffix(')')) {
        inner.trim().to_owned()
    } else {
        trimmed
    }
}

unsafe extern "C" fn cb_send_char(msg: *mut c_char, _ident: c_int, userdata: *mut c_void) -> c_int {
    if msg.is_null() || userdata.is_null() {
        return 0;
    }
    let state = unsafe { &*(userdata as *const CallbackState) };
    if state.detached.load(Ordering::Acquire) {
        return 0;
    }
    let text = unsafe { CStr::from_ptr(msg) }.to_string_lossy();
    if let Ok(mut guard) = state.inner.lock() {
        if text.starts_with("stderr ") {
            let trimmed = text.trim_start_matches("stderr ").trim();
            if trimmed.to_ascii_lowercase().contains("error")
                || trimmed.to_ascii_lowercase().contains("aborted")
            {
                guard.last_error = Some(trimmed.to_owned());
            }
        }
        if guard.stdout_tail.len() >= 32 {
            guard.stdout_tail.remove(0);
        }
        guard.stdout_tail.push(text.into_owned());
    }
    0
}

unsafe extern "C" fn cb_send_stat(
    _msg: *mut c_char,
    _ident: c_int,
    _userdata: *mut c_void,
) -> c_int {
    0
}

unsafe extern "C" fn cb_controlled_exit(
    status: c_int,
    _immediate: bool,
    quit_exit: bool,
    _ident: c_int,
    userdata: *mut c_void,
) -> c_int {
    if !userdata.is_null() {
        let state = unsafe { &*(userdata as *const CallbackState) };
        if state.detached.load(Ordering::Acquire) {
            return status;
        }
        if let Ok(mut guard) = state.inner.lock()
            && !quit_exit
        {
            guard.last_error = Some(format!("ngspice controlled exit status={status}"));
        }
        state.wakeup.mark_bg_finished();
    }
    status
}

unsafe extern "C" fn cb_send_init_data(
    info: *mut VecInfoAll,
    _ident: c_int,
    userdata: *mut c_void,
) -> c_int {
    if info.is_null() || userdata.is_null() {
        return 0;
    }
    let state = unsafe { &*(userdata as *const CallbackState) };
    if state.detached.load(Ordering::Acquire) {
        return 0;
    }
    let info_ref = unsafe { &*info };
    let count = info_ref.veccount.max(0) as usize;
    let vecs_ptr = info_ref.vecs;
    if vecs_ptr.is_null() {
        return 0;
    }
    let mut names = Vec::with_capacity(count);
    for i in 0..count {
        let vp = unsafe { *vecs_ptr.add(i) };
        if vp.is_null() {
            names.push(String::new());
        } else {
            let vref = unsafe { &*vp };
            let s = if vref.vecname.is_null() {
                String::new()
            } else {
                unsafe { CStr::from_ptr(vref.vecname) }
                    .to_string_lossy()
                    .to_ascii_lowercase()
            };
            names.push(s);
        }
    }

    if let Ok(mut guard) = state.inner.lock() {
        guard.scale_idx = names.iter().position(|n| n == "time").or(Some(0));
        guard.probe_indices.clear();
        guard.missing_probe = None;
        let requested = guard.requested_probes.clone();
        for probe in &requested {
            let norm = normalize_probe(probe);
            let found = names
                .iter()
                .position(|n| n == &norm || normalize_probe(n) == norm);
            if let Some(idx) = found {
                guard.probe_indices.push(idx);
            } else {
                guard.missing_probe = Some(probe.clone());
                break;
            }
        }
        let n = guard.probe_indices.len();
        guard.scratch_vals.resize(n, 0.0);
    }
    0
}

unsafe extern "C" fn cb_send_data(
    data: *mut VecValuesAll,
    count: c_int,
    _ident: c_int,
    userdata: *mut c_void,
) -> c_int {
    if data.is_null() || userdata.is_null() || count <= 0 {
        return 0;
    }
    let state = unsafe { &*(userdata as *const CallbackState) };
    if state.detached.load(Ordering::Acquire) {
        return 0;
    }
    let all = unsafe { &*data };
    let veccount = all.veccount.max(0) as usize;
    if all.vecsa.is_null() || veccount == 0 {
        return 0;
    }
    if let Ok(mut guard) = state.inner.lock() {
        if guard.missing_probe.is_some() {
            return 0;
        }
        let scale_idx = guard.scale_idx.unwrap_or(0);
        if scale_idx >= veccount {
            return 0;
        }
        let t_ptr = unsafe { *all.vecsa.add(scale_idx) };
        if t_ptr.is_null() {
            return 0;
        }
        let t = unsafe { (*t_ptr).creal };

        let n_probes = guard.probe_indices.len();
        for p in 0..n_probes {
            let idx = guard.probe_indices[p];
            if idx >= veccount {
                return 0;
            }
            let vp = unsafe { *all.vecsa.add(idx) };
            if vp.is_null() {
                return 0;
            }
            guard.scratch_vals[p] = unsafe { (*vp).creal };
        }
        if guard.first_sample_at.is_none() {
            guard.first_sample_at = Some(Instant::now());
        }
        guard.total_points += 1;

        if let Some(mut sink) = guard.sink.take() {
            sink.on_point(t, &guard.scratch_vals);
            guard.sink = Some(sink);
        }
    }
    0
}

unsafe extern "C" fn cb_bg_thread_running(
    not_running: bool,
    _ident: c_int,
    userdata: *mut c_void,
) -> c_int {
    if !userdata.is_null() {
        let state = unsafe { &*(userdata as *const CallbackState) };
        // Latch on every callback notification (start and finish) regardless of which
        // ngspice command initiated the background thread.
        state.bg_ever_started.store(true, Ordering::Release);
        if not_running {
            state.wakeup.mark_bg_finished();
        }
    }
    0
}

fn may_spawn_ngspice_thread(cmd: &str) -> bool {
    cmd.split([';', '\n', '\r']).any(|stmt| {
        stmt.split_whitespace().next().is_some_and(|tok| {
            let lower = tok.to_ascii_lowercase();
            (lower.starts_with("bg_") && !matches!(lower.as_str(), "bg_halt" | "bg_pstop"))
                || matches!(lower.as_str(), "controlswait" | "source")
        })
    })
}

fn netlist_has_control_block(lines: &[String]) -> bool {
    lines.iter().any(|line| {
        line.trim_start()
            .to_ascii_lowercase()
            .starts_with(".control")
    })
}

/// Exclusive process-global handle to dynamically loaded `libngspice`.
///
/// Intentionally `!Sync` (`PhantomData<*const ()>` + no `Sync` impl) so every FFI call is
/// strictly serialized through `&mut self`.
pub struct NgspiceLib {
    // Dropped first when safe (`!bg_ever_started`) so `libngspice` unloads while `state`
    // is still live and `guard` still holds `ACTIVE_INSTANCE`.
    lib: Option<Library>,
    fn_command: NgSpiceCommandFn,
    fn_circ: NgSpiceCircFn,
    fn_running: NgSpiceRunningFn,
    fn_get_vec_info: NgGetVecInfoFn,
    state: Option<Box<CallbackState>>,
    bg_active: bool,
    bg_ever_started: bool,
    guard: Option<InstanceGuard>,
    _not_sync: PhantomData<std::cell::Cell<()>>,
}

pub struct TranRunStats {
    pub total_points: u64,
    pub first_sample_at: Option<Instant>,
    pub halted_by_cancel: bool,
}

impl NgspiceLib {
    /// Dynamically load `libngspice` and initialize callbacks. Fails if another `NgspiceLib`
    /// instance is already active in this process.
    pub fn load(path: &Path) -> Result<Self, String> {
        let guard = InstanceGuard::acquire()?;

        // SAFETY: Loading a user/harness-specified shared library matching `sharedspice.h`.
        let lib = unsafe { Library::new(path) }
            .map_err(|e| format!("failed to load {}: {e}", path.display()))?;

        unsafe {
            let fn_init: NgSpiceInitFn = *lib
                .get::<NgSpiceInitFn>(b"ngSpice_Init\0")
                .map_err(|e| format!("missing ngSpice_Init: {e}"))?;
            let fn_command: NgSpiceCommandFn =
                *lib.get::<NgSpiceCommandFn>(b"ngSpice_Command\0")
                    .map_err(|e| format!("missing ngSpice_Command: {e}"))?;
            let fn_circ: NgSpiceCircFn = *lib
                .get::<NgSpiceCircFn>(b"ngSpice_Circ\0")
                .map_err(|e| format!("missing ngSpice_Circ: {e}"))?;
            let fn_running: NgSpiceRunningFn =
                *lib.get::<NgSpiceRunningFn>(b"ngSpice_running\0")
                    .map_err(|e| format!("missing ngSpice_running: {e}"))?;
            let fn_get_vec_info: NgGetVecInfoFn =
                *lib.get::<NgGetVecInfoFn>(b"ngGet_Vec_Info\0")
                    .map_err(|e| format!("missing ngGet_Vec_Info: {e}"))?;

            let state = Box::new(CallbackState {
                detached: AtomicBool::new(false),
                bg_ever_started: AtomicBool::new(false),
                wakeup: BgWakeup::new(),
                inner: Mutex::new(CallbackInner::new()),
            });

            // Note: Do NOT call `ngSpice_nospinit()` / `ngSpice_nospiceinit()` before
            // `ngSpice_Init()` on Windows MinGW builds using Win32 threads: in upstream
            // ngspice-47 (`src/sharedspice.c:806-820, 903-907`), `cp_vset` allocates via
            // `allocMutex` before `ngSpice_Init` initializes the `CRITICAL_SECTION`,
            // causing a 0xC0000005 access violation.
            let ud = (&*state) as *const CallbackState as *mut c_void;
            let rc = fn_init(
                Some(cb_send_char),
                Some(cb_send_stat),
                Some(cb_controlled_exit),
                Some(cb_send_data),
                Some(cb_send_init_data),
                Some(cb_bg_thread_running),
                ud,
            );
            if rc != 0 {
                return Err(format!("ngSpice_Init returned {rc}"));
            }

            Ok(Self {
                lib: Some(lib),
                fn_command,
                fn_circ,
                fn_running,
                fn_get_vec_info,
                state: Some(state),
                bg_active: false,
                bg_ever_started: false,
                guard: Some(guard),
                _not_sync: PhantomData,
            })
        }
    }

    fn state_ref(&self) -> &CallbackState {
        self.state.as_deref().expect("state live until Drop")
    }

    /// Returns a thread-safe wakeup handle that a control thread may signal to interrupt
    /// `wait_tran_bg_cancellable` without touching any `libngspice` FFI symbol.
    pub fn wakeup_handle(&self) -> BgWakeup {
        self.state_ref().wakeup.clone()
    }

    fn mark_bg_ever_started(&mut self) {
        self.bg_ever_started = true;
        if let Some(state) = self.state.as_deref() {
            state.bg_ever_started.store(true, Ordering::Release);
        }
    }

    fn command(&mut self, cmd: &str) -> Result<(), String> {
        if may_spawn_ngspice_thread(cmd) {
            self.mark_bg_ever_started();
        }
        let cstr = CString::new(cmd).map_err(|e| format!("invalid command string: {e}"))?;
        let rc = unsafe { (self.fn_command)(cstr.as_ptr() as *mut c_char) };
        if self.is_running() {
            self.mark_bg_ever_started();
        }
        if rc != 0 {
            return Err(format!("ngSpice_Command({cmd:?}) returned {rc}"));
        }
        Ok(())
    }

    pub fn is_running(&self) -> bool {
        unsafe { (self.fn_running)() }
    }

    pub fn bg_halt(&mut self) -> Result<(), String> {
        self.command("bg_halt")
    }

    fn reset_circuit(&mut self) -> Result<(), String> {
        let _ = self.command("destroy all");
        let _ = self.command("remcirc");
        Ok(())
    }

    fn load_circuit(&mut self, lines: &[String]) -> Result<(), String> {
        if netlist_has_control_block(lines) {
            self.mark_bg_ever_started();
        }
        let cstrings: Vec<CString> = lines
            .iter()
            .map(|l| CString::new(l.as_str()).map_err(|e| format!("nul in netlist line: {e}")))
            .collect::<Result<_, _>>()?;
        let mut ptrs: Vec<*mut c_char> =
            cstrings.iter().map(|s| s.as_ptr() as *mut c_char).collect();
        ptrs.push(std::ptr::null_mut());
        let rc = unsafe { (self.fn_circ)(ptrs.as_mut_ptr()) };
        if rc != 0 {
            return Err(format!("ngSpice_Circ returned {rc}"));
        }
        Ok(())
    }

    /// Run a DC operating point (`op`) and read the requested probe vector values (`SINV7`:
    /// missing vectors return an explicit error, never `0.0`).
    pub fn run_op(&mut self, netlist: &[String], probes: &[String]) -> Result<Vec<f64>, String> {
        self.reset_circuit()?;
        {
            let mut guard = self.state_ref().inner.lock().map_err(|_| "lock poisoned")?;
            guard.requested_probes.clear();
            guard.sink = None;
            guard.last_error = None;
            guard.missing_probe = None;
        }
        self.load_circuit(netlist)?;
        self.command("op")?;
        let mut out = Vec::with_capacity(probes.len());
        for p in probes {
            let norm = normalize_probe(p);
            let cname = CString::new(norm.as_str()).map_err(|e| format!("bad probe {p}: {e}"))?;
            let info_ptr = unsafe { (self.fn_get_vec_info)(cname.as_ptr() as *mut c_char) };
            if info_ptr.is_null() {
                return Err(format!("missing probe vector '{p}' (SINV7)"));
            }
            let info = unsafe { &*info_ptr };
            if info.v_length <= 0 || info.v_realdata.is_null() {
                return Err(format!("probe vector '{p}' has no real data (SINV7)"));
            }
            let val = unsafe { *info.v_realdata };
            out.push(val);
        }
        Ok(out)
    }

    /// Start a background transient (`bg_run`) streaming samples to `sink`.
    pub fn start_tran_bg(
        &mut self,
        netlist: &[String],
        probes: &[String],
        sink: Box<dyn SampleSink>,
    ) -> Result<(), String> {
        self.reset_circuit()?;
        self.state_ref().wakeup.reset_for_run();
        {
            let mut guard = self.state_ref().inner.lock().map_err(|_| "lock poisoned")?;
            guard.requested_probes = probes.to_vec();
            guard.scale_idx = None;
            guard.probe_indices.clear();
            guard.missing_probe = None;
            guard.scratch_vals.clear();
            guard.sink = Some(sink);
            guard.total_points = 0;
            guard.first_sample_at = None;
            guard.last_error = None;
        }
        self.load_circuit(netlist)?;
        self.bg_active = true;
        self.mark_bg_ever_started();
        if let Err(e) = self.command("bg_run") {
            self.bg_active = false;
            self.state_ref().wakeup.mark_bg_finished();
            return Err(e);
        }
        Ok(())
    }

    /// Wait for the active `bg_run` to complete, invoking `bg_halt()` on this exclusive owner
    /// thread as soon as `wakeup_handle().request_cancel()` is signalled.
    pub fn wait_tran_bg_cancellable(&mut self, timeout: Duration) -> Result<TranRunStats, String> {
        let deadline = Instant::now() + timeout;
        let wakeup = self.state_ref().wakeup.clone();
        let mut cancel_attempted = false;
        let mut halted_by_cancel = false;

        loop {
            let (bg_done, want_cancel) = {
                let mut g = wakeup.inner.0.lock().map_err(|_| "wakeup lock poisoned")?;
                loop {
                    if g.bg_finished {
                        break (true, false);
                    }
                    if g.cancel_requested && !cancel_attempted {
                        break (false, true);
                    }
                    let now = Instant::now();
                    if now >= deadline {
                        return Err("timed out waiting for ngspice bg_run".into());
                    }
                    let wait_for = (deadline - now).min(Duration::from_millis(5));
                    let (next, _) = wakeup
                        .inner
                        .1
                        .wait_timeout(g, wait_for)
                        .map_err(|_| "condvar poisoned")?;
                    g = next;
                }
            };

            if want_cancel {
                cancel_attempted = true;
                // Only credit cancellation when `bg_run` is still running and `bg_halt` succeeds.
                let already_finished = wakeup.inner.0.lock().map(|g| g.bg_finished).unwrap_or(true);
                if !already_finished && self.is_running() && self.bg_halt().is_ok() {
                    halted_by_cancel = true;
                }
                continue;
            }
            if bg_done {
                break;
            }
        }

        // Ensure ngspice's own running flag has also cleared.
        while self.is_running() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        self.bg_active = false;

        let mut guard = self.state_ref().inner.lock().map_err(|_| "lock poisoned")?;
        let missing = guard.missing_probe.clone();
        let total_points = guard.total_points;
        let first_sample_at = guard.first_sample_at;
        let last_error = guard.last_error.clone();
        guard.sink = None;
        drop(guard);

        if let Some(m) = missing {
            return Err(format!("missing probe vector '{m}' (SINV7)"));
        }
        if let Some(err) = last_error
            && total_points == 0
        {
            return Err(err);
        }
        Ok(TranRunStats {
            total_points,
            first_sample_at,
            halted_by_cancel,
        })
    }

    /// Wait for the active `bg_run` to complete (or after an explicit `bg_halt()`).
    pub fn wait_tran_bg(&mut self, timeout: Duration) -> Result<TranRunStats, String> {
        self.wait_tran_bg_cancellable(timeout)
    }

    /// Synchronous transient (`run` on current thread) — avoids bg thread startup overhead
    /// for pure in-process comparison where cancellation isn't exercised.
    pub fn run_tran_sync(
        &mut self,
        netlist: &[String],
        probes: &[String],
        sink: Box<dyn SampleSink>,
    ) -> Result<TranRunStats, String> {
        self.reset_circuit()?;
        {
            let mut guard = self.state_ref().inner.lock().map_err(|_| "lock poisoned")?;
            guard.requested_probes = probes.to_vec();
            guard.scale_idx = None;
            guard.probe_indices.clear();
            guard.missing_probe = None;
            guard.scratch_vals.clear();
            guard.sink = Some(sink);
            guard.total_points = 0;
            guard.first_sample_at = None;
            guard.last_error = None;
        }
        self.load_circuit(netlist)?;
        self.command("run")?;
        let mut guard = self.state_ref().inner.lock().map_err(|_| "lock poisoned")?;
        let missing = guard.missing_probe.clone();
        let total_points = guard.total_points;
        let first_sample_at = guard.first_sample_at;
        guard.sink = None;
        drop(guard);
        if let Some(m) = missing {
            return Err(format!("missing probe vector '{m}' (SINV7)"));
        }
        Ok(TranRunStats {
            total_points,
            first_sample_at,
            halted_by_cancel: false,
        })
    }
}

impl Drop for NgspiceLib {
    fn drop(&mut self) {
        let mut ever_bg = self.bg_active
            || self.bg_ever_started
            || self
                .state
                .as_deref()
                .is_some_and(|s| s.bg_ever_started.load(Ordering::Acquire))
            || self.is_running();

        // 1. Stop and wait for any active ngspice background thread BEFORE detaching.
        if self.bg_active || self.is_running() {
            let _halt_ok = self.bg_halt().is_ok();
            let deadline = Instant::now() + Duration::from_secs(2);
            if let Some(state) = self.state.as_deref() {
                let wakeup = &state.wakeup;
                if let Ok(mut g) = wakeup.inner.0.lock() {
                    while !g.bg_finished && Instant::now() < deadline {
                        let rem = deadline.saturating_duration_since(Instant::now());
                        match wakeup
                            .inner
                            .1
                            .wait_timeout(g, rem.min(Duration::from_millis(5)))
                        {
                            Ok((next, _)) => g = next,
                            Err(_) => break,
                        }
                    }
                }
            }
            while self.is_running() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(1));
            }
            self.bg_active = false;
        }

        // 2. Detach callbacks and clear sinks while CallbackState is still alive.
        if let Some(state) = self.state.as_deref() {
            state.detached.store(true, Ordering::Release);
            if let Ok(mut guard) = state.inner.lock() {
                guard.sink = None;
            }
        }

        ever_bg |= self
            .state
            .as_deref()
            .is_some_and(|s| s.bg_ever_started.load(Ordering::Acquire))
            || self.is_running();

        // 3. Only unload `libngspice`, free `CallbackState`, and release `ACTIVE_INSTANCE`
        //    when no background solver thread was ever started (`!ever_bg`).
        //    Upstream ngspice-47 fires `cb_bg_thread_running` before the solver thread returns
        //    and spawns detached pthreads on Unix without a join API, so pinning avoids any
        //    possibility of unloading code or freeing state beneath an exiting thread.
        if !ever_bg {
            drop(self.lib.take());
            drop(self.state.take());
            drop(self.guard.take());
        } else {
            self.bg_ever_started = true;
            std::mem::forget(self.lib.take());
            std::mem::forget(self.state.take());
            std::mem::forget(self.guard.take());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn may_spawn_and_control_detection() {
        assert!(may_spawn_ngspice_thread("bg_run"));
        assert!(may_spawn_ngspice_thread("  BG_OP "));
        assert!(may_spawn_ngspice_thread("bg_ac"));
        assert!(may_spawn_ngspice_thread("bg_resume"));
        assert!(may_spawn_ngspice_thread("bg_step"));
        assert!(may_spawn_ngspice_thread("bg_trun"));
        assert!(may_spawn_ngspice_thread("controlswait"));
        assert!(may_spawn_ngspice_thread("source helper.sp"));
        assert!(may_spawn_ngspice_thread("echo start; bg_op"));
        assert!(!may_spawn_ngspice_thread("op"));
        assert!(!may_spawn_ngspice_thread("run"));
        assert!(!may_spawn_ngspice_thread("bg_halt"));
        assert!(!may_spawn_ngspice_thread("bg_pstop"));
        assert!(!may_spawn_ngspice_thread("destroy all"));

        assert!(netlist_has_control_block(&[
            "* test".into(),
            "  .CONTROL ".into(),
            "op".into(),
            ".endc".into()
        ]));
        assert!(!netlist_has_control_block(&[
            "* comment".into(),
            "R1 1 0 1k".into(),
            ".end".into()
        ]));
    }

    #[test]
    fn cb_bg_thread_running_latches_bg_ever_started() {
        let mut state = CallbackState {
            detached: AtomicBool::new(false),
            bg_ever_started: AtomicBool::new(false),
            wakeup: BgWakeup::new(),
            inner: Mutex::new(CallbackInner::new()),
        };
        let ud = (&mut state as *mut CallbackState).cast::<c_void>();
        unsafe {
            cb_bg_thread_running(false, 0, ud);
        }
        assert!(state.bg_ever_started.load(Ordering::Acquire));

        state.bg_ever_started.store(false, Ordering::Release);
        state.wakeup.reset_for_run();
        unsafe {
            cb_bg_thread_running(true, 0, ud);
        }
        assert!(state.bg_ever_started.load(Ordering::Acquire));
    }
}
