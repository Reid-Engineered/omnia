//! Dynamic FFI loader for BSD-3-Clause `libngspice` (`ngspice.dll` / `libngspice.so`)
//! matching `src/include/ngspice/sharedspice.h` from ngspice 47.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::Path;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use libloading::{Library, Symbol};

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
type NgSpiceNoInitFn = unsafe extern "C" fn() -> c_int;
type NgGetVecInfoFn = unsafe extern "C" fn(*mut c_char) -> *mut VectorInfo;

/// Callback sink invoked on every accepted transient point inside `SendData`.
pub trait SampleSink: Send {
    fn on_point(&mut self, t: f64, values: &[f64]);
}

/// Shared state bridged via `userData` pointer to `libngspice` C callbacks.
struct CallbackState {
    inner: Mutex<CallbackInner>,
    bg_cv: Condvar,
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
    bg_finished: bool,
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
            bg_finished: false,
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
        if let Ok(mut guard) = state.inner.lock() {
            if !quit_exit {
                guard.last_error = Some(format!("ngspice controlled exit status={status}"));
            }
            guard.bg_finished = true;
            state.bg_cv.notify_all();
        }
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

        // Split borrow via temporary take to avoid borrow conflict with scratch_vals.
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
        if let Ok(mut guard) = state.inner.lock()
            && not_running
        {
            guard.bg_finished = true;
            state.bg_cv.notify_all();
        }
    }
    0
}

/// Loaded `libngspice` dynamic library handle with pinned callback state.
pub struct NgspiceLib {
    _lib: Library,
    fn_init: NgSpiceInitFn,
    fn_command: NgSpiceCommandFn,
    fn_circ: NgSpiceCircFn,
    fn_running: NgSpiceRunningFn,
    fn_nospinit: Option<NgSpiceNoInitFn>,
    fn_nospiceinit: Option<NgSpiceNoInitFn>,
    fn_get_vec_info: NgGetVecInfoFn,
    state: Box<CallbackState>,
}

// SAFETY: `libngspice` has process-global state; callers synchronize commands through `&self`
// (and `bg_halt` is explicitly documented by ngspice as callable from the primary thread while
// the background thread runs).
unsafe impl Send for NgspiceLib {}
unsafe impl Sync for NgspiceLib {}

pub struct TranRunStats {
    pub total_points: u64,
    pub first_sample_at: Option<Instant>,
}

impl NgspiceLib {
    /// Dynamically load `libngspice` and initialize callbacks (`nospinit` + `nospiceinit`).
    pub fn load(path: &Path) -> Result<Self, String> {
        // SAFETY: Loading a user/harness-specified shared library matching `sharedspice.h`.
        let lib = unsafe { Library::new(path) }
            .map_err(|e| format!("failed to load {}: {e}", path.display()))?;

        unsafe {
            let fn_init: Symbol<'_, NgSpiceInitFn> = lib
                .get(b"ngSpice_Init\0")
                .map_err(|e| format!("missing ngSpice_Init: {e}"))?;
            let fn_command: Symbol<'_, NgSpiceCommandFn> = lib
                .get(b"ngSpice_Command\0")
                .map_err(|e| format!("missing ngSpice_Command: {e}"))?;
            let fn_circ: Symbol<'_, NgSpiceCircFn> = lib
                .get(b"ngSpice_Circ\0")
                .map_err(|e| format!("missing ngSpice_Circ: {e}"))?;
            let fn_running: Symbol<'_, NgSpiceRunningFn> = lib
                .get(b"ngSpice_running\0")
                .map_err(|e| format!("missing ngSpice_running: {e}"))?;
            let fn_get_vec_info: Symbol<'_, NgGetVecInfoFn> = lib
                .get(b"ngGet_Vec_Info\0")
                .map_err(|e| format!("missing ngGet_Vec_Info: {e}"))?;
            let fn_nospinit = lib
                .get::<NgSpiceNoInitFn>(b"ngSpice_nospinit\0")
                .ok()
                .map(|s| *s);
            let fn_nospiceinit = lib
                .get::<NgSpiceNoInitFn>(b"ngSpice_nospiceinit\0")
                .ok()
                .map(|s| *s);

            let me = Self {
                fn_init: *fn_init,
                fn_command: *fn_command,
                fn_circ: *fn_circ,
                fn_running: *fn_running,
                fn_nospinit,
                fn_nospiceinit,
                fn_get_vec_info: *fn_get_vec_info,
                _lib: lib,
                state: Box::new(CallbackState {
                    inner: Mutex::new(CallbackInner::new()),
                    bg_cv: Condvar::new(),
                }),
            };

            // Note: Do NOT call `ngSpice_nospinit()` / `ngSpice_nospiceinit()` before
            // `ngSpice_Init()` on Windows MinGW builds using Win32 threads: in upstream
            // ngspice-47 (`src/sharedspice.c:806-820, 903-907`), `cp_vset` allocates via
            // `allocMutex` before `ngSpice_Init` initializes the `CRITICAL_SECTION`,
            // causing a 0xC0000005 access violation. Because Omnia ships no `spinit` or
            // `.cm` codemodels alongside `libngspice`, omitting them is clean on both OSes.
            let _ = (me.fn_nospinit, me.fn_nospiceinit);
            let ud = (&*me.state) as *const CallbackState as *mut c_void;
            let rc = (me.fn_init)(
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
            Ok(me)
        }
    }

    pub fn command(&self, cmd: &str) -> Result<(), String> {
        let cstr = CString::new(cmd).map_err(|e| format!("invalid command string: {e}"))?;
        let rc = unsafe { (self.fn_command)(cstr.as_ptr() as *mut c_char) };
        if rc != 0 {
            return Err(format!("ngSpice_Command({cmd:?}) returned {rc}"));
        }
        Ok(())
    }

    pub fn is_running(&self) -> bool {
        unsafe { (self.fn_running)() }
    }

    pub fn bg_halt(&self) -> Result<(), String> {
        self.command("bg_halt")
    }

    pub fn reset_circuit(&self) -> Result<(), String> {
        let _ = self.command("destroy all");
        let _ = self.command("remcirc");
        Ok(())
    }

    pub fn load_circuit(&self, lines: &[String]) -> Result<(), String> {
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
    pub fn run_op(&self, netlist: &[String], probes: &[String]) -> Result<Vec<f64>, String> {
        self.reset_circuit()?;
        {
            let mut guard = self.state.inner.lock().map_err(|_| "lock poisoned")?;
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
        &self,
        netlist: &[String],
        probes: &[String],
        sink: Box<dyn SampleSink>,
    ) -> Result<(), String> {
        self.reset_circuit()?;
        {
            let mut guard = self.state.inner.lock().map_err(|_| "lock poisoned")?;
            guard.requested_probes = probes.to_vec();
            guard.scale_idx = None;
            guard.probe_indices.clear();
            guard.missing_probe = None;
            guard.scratch_vals.clear();
            guard.sink = Some(sink);
            guard.total_points = 0;
            guard.first_sample_at = None;
            guard.bg_finished = false;
            guard.last_error = None;
        }
        self.load_circuit(netlist)?;
        self.command("bg_run")?;
        Ok(())
    }

    /// Wait for the active `bg_run` to complete (either naturally or after `bg_halt`).
    pub fn wait_tran_bg(&self, timeout: Duration) -> Result<TranRunStats, String> {
        let deadline = Instant::now() + timeout;
        let mut guard = self.state.inner.lock().map_err(|_| "lock poisoned")?;
        while !guard.bg_finished {
            let now = Instant::now();
            if now >= deadline {
                return Err("timed out waiting for ngspice bg_run".into());
            }
            let wait_for = (deadline - now).min(Duration::from_millis(5));
            let (g, _) = self
                .state
                .bg_cv
                .wait_timeout(guard, wait_for)
                .map_err(|_| "condvar poisoned")?;
            guard = g;
        }
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
        })
    }

    /// Synchronous transient (`run` on current thread) — avoids bg thread startup overhead
    /// for pure in-process comparison where cancellation isn't exercised.
    pub fn run_tran_sync(
        &self,
        netlist: &[String],
        probes: &[String],
        sink: Box<dyn SampleSink>,
    ) -> Result<TranRunStats, String> {
        self.reset_circuit()?;
        {
            let mut guard = self.state.inner.lock().map_err(|_| "lock poisoned")?;
            guard.requested_probes = probes.to_vec();
            guard.scale_idx = None;
            guard.probe_indices.clear();
            guard.missing_probe = None;
            guard.scratch_vals.clear();
            guard.sink = Some(sink);
            guard.total_points = 0;
            guard.first_sample_at = None;
            guard.bg_finished = false;
            guard.last_error = None;
        }
        self.load_circuit(netlist)?;
        self.command("run")?;
        let mut guard = self.state.inner.lock().map_err(|_| "lock poisoned")?;
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
        })
    }
}
