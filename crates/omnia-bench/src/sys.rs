//! Process memory and CPU time from the operating system. Windows only for now; other
//! platforms report `None`.

use std::time::Duration;

/// Process memory in bytes.
#[derive(Clone, Copy, Debug)]
pub struct Memory {
    /// Private committed bytes (Task Manager's "Commit size"). GPU memory is not included.
    pub private_bytes: u64,
    pub working_set_bytes: u64,
    pub peak_working_set_bytes: u64,
}

/// User plus kernel CPU time this process has used.
pub fn cpu_time() -> Option<Duration> {
    imp::cpu_time()
}

pub fn memory() -> Option<Memory> {
    imp::memory()
}

#[cfg(windows)]
mod imp {
    use super::Memory;
    use std::ffi::c_void;
    use std::time::Duration;

    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }

    impl FileTime {
        fn hundred_ns(&self) -> u64 {
            (u64::from(self.high) << 32) | u64::from(self.low)
        }
    }

    #[repr(C)]
    #[derive(Default)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    // kernel32 exports these on every supported Windows version (K32* since Windows 7).
    #[link(name = "kernel32")]
    unsafe extern "system" {
        safe fn GetCurrentProcess() -> *mut c_void;
        fn GetProcessTimes(
            process: *mut c_void,
            creation: *mut FileTime,
            exit: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
        fn K32GetProcessMemoryInfo(
            process: *mut c_void,
            counters: *mut ProcessMemoryCounters,
            cb: u32,
        ) -> i32;
    }

    pub fn cpu_time() -> Option<Duration> {
        let (mut creation, mut exit, mut kernel, mut user) = Default::default();
        // SAFETY: the pseudo-handle is always valid; the out-pointers are live locals.
        let ok = unsafe {
            GetProcessTimes(
                GetCurrentProcess(),
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            )
        };
        let _ = (creation, exit);
        (ok != 0).then(|| Duration::from_nanos((kernel.hundred_ns() + user.hundred_ns()) * 100))
    }

    pub fn memory() -> Option<Memory> {
        let mut counters = ProcessMemoryCounters {
            cb: size_of::<ProcessMemoryCounters>() as u32,
            ..Default::default()
        };
        // SAFETY: `counters` is a correctly sized, live PROCESS_MEMORY_COUNTERS.
        let ok =
            unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) };
        (ok != 0).then_some(Memory {
            private_bytes: counters.pagefile_usage as u64,
            working_set_bytes: counters.working_set_size as u64,
            peak_working_set_bytes: counters.peak_working_set_size as u64,
        })
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Memory;
    use std::time::Duration;

    pub fn cpu_time() -> Option<Duration> {
        None
    }

    pub fn memory() -> Option<Memory> {
        None
    }
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn reads_process_counters() {
        let memory = super::memory().expect("memory info");
        assert!(memory.private_bytes > 0 && memory.working_set_bytes > 0);
        let before = super::cpu_time().expect("cpu time");
        let mut x = 0_u64;
        for i in 0..20_000_000_u64 {
            x = std::hint::black_box(x.wrapping_add(i));
        }
        assert!(super::cpu_time().unwrap() >= before);
    }
}
