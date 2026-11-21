//! Diagnostic only (T2.7 paint-stall investigation): per-paint thread scheduling counters.
//!
//! With `OMNIA_BENCH_PAINT_PROBE=1` on Linux, every `SceneElement` paint records: wall time,
//! thread CPU time (`CLOCK_THREAD_CPUTIME_ID`), page faults and voluntary and involuntary
//! context switches (`getrusage(RUSAGE_THREAD)`), run-queue wait and timeslices
//! (`/proc/thread-self/schedstat`), the CPU at start and end (`sched_getcpu`), and the end
//! CPU's `scaling_cur_freq` (an unreliable instant reading under intel_pstate, kept for
//! context), and the thread's user-space cycles, instructions and reference cycles
//! (`perf_event_open`, unprivileged at perf_event_paranoid 2): cycles / reference cycles is
//! the core clock while running, instructions per cycle shows contention. Wall time well above CPU time means the thread was not
//! running (run-queue wait says whether it was runnable); CPU time well above the usual
//! paint, with no faults or waits, means it ran slowly. Files are opened once and re-read with
//! `pread` into stack buffers, and samples go into a vector reserved up front, so a paint
//! allocates nothing. Never part of a gate: the reads add a few tens of microseconds per paint.

#[cfg(target_os = "linux")]
mod linux {
    use serde_json::{Value, json};
    use std::cell::RefCell;
    use std::time::Instant;

    #[derive(Clone, Copy, Default)]
    struct Counters {
        cpu_ns: u64,
        wait_ns: u64,
        slices: u64,
        minflt: u64,
        majflt: u64,
        voluntary: u64,
        involuntary: u64,
        cpu: u32,
        /// User-space cycles, instructions and reference cycles; `u64::MAX` when unavailable.
        hw: [u64; 3],
        /// Hardware counters were time-multiplexed (enabled time != running time).
        multiplexed: bool,
    }

    #[derive(Clone, Copy)]
    struct Sample {
        wall_ns: u64,
        start: Counters,
        end: Counters,
        freq_khz: u64,
    }

    struct Files {
        schedstat: std::fs::File,
        freq: Vec<Option<std::fs::File>>,
        /// perf_event fds for this thread: cycles, instructions, ref-cycles (user space only).
        hw: [Option<std::fs::File>; 3],
    }

    /// Opens a user-space-only hardware counter for the calling thread (perf_event_open).
    /// Works at perf_event_paranoid 2 without privileges. `perf_event_attr` is laid out by
    /// hand: libc has the syscall number but not the struct (PERF_ATTR_SIZE_VER8, 136 bytes).
    fn hw_counter(config: u64) -> Option<std::fs::File> {
        use std::os::fd::FromRawFd;
        const PERF_TYPE_HARDWARE: u64 = 0;
        const ATTR_SIZE: u64 = 136;
        const READ_TIMES: u64 = 1 | 2; // TOTAL_TIME_ENABLED | TOTAL_TIME_RUNNING
        const EXCLUDE_KERNEL_HV: u64 = (1 << 5) | (1 << 6);
        const PERF_FLAG_FD_CLOEXEC: libc::c_ulong = 1 << 3;
        let mut attr = [0u64; 17];
        attr[0] = PERF_TYPE_HARDWARE | (ATTR_SIZE << 32); // type: u32, size: u32
        attr[1] = config;
        attr[4] = READ_TIMES; // read_format
        attr[5] = EXCLUDE_KERNEL_HV; // flags bitfield
        // SAFETY: attr is a zero-initialised, correctly sized perf_event_attr.
        let fd = unsafe {
            libc::syscall(
                libc::SYS_perf_event_open,
                attr.as_ptr(),
                0,
                -1,
                -1,
                PERF_FLAG_FD_CLOEXEC,
            )
        };
        // SAFETY: a non-negative return is a new fd we own.
        (fd >= 0).then(|| unsafe { std::fs::File::from_raw_fd(fd as i32) })
    }

    struct Probe {
        files: Files,
        samples: Vec<Sample>,
        dropped: u64,
        open: Option<(Instant, Counters)>,
    }

    thread_local! {
        static PROBE: RefCell<Option<Probe>> = const { RefCell::new(None) };
    }

    /// Starts recording on this thread if `OMNIA_BENCH_PAINT_PROBE` is set and the platform
    /// supports it. Returns whether it is recording.
    pub(super) fn enable(capacity: usize) -> bool {
        if std::env::var_os("OMNIA_BENCH_PAINT_PROBE").is_none() {
            return false;
        }
        {
            use std::fs::File;
            let Ok(schedstat) = File::open("/proc/thread-self/schedstat") else {
                return false;
            };
            let cpus = std::thread::available_parallelism().map_or(1, |n| n.get());
            let freq = (0..cpus)
                .map(|cpu| {
                    File::open(format!(
                        "/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_cur_freq"
                    ))
                    .ok()
                })
                .collect();
            PROBE.with_borrow_mut(|probe| {
                *probe = Some(Probe {
                    files: Files {
                        schedstat,
                        freq,
                        // PERF_COUNT_HW_CPU_CYCLES, _INSTRUCTIONS, _REF_CPU_CYCLES.
                        hw: [hw_counter(0), hw_counter(1), hw_counter(9)],
                    },
                    samples: Vec::with_capacity(capacity),
                    dropped: 0,
                    open: None,
                })
            });
            true
        }
    }

    pub(super) fn begin() {
        PROBE.with_borrow_mut(|probe| {
            if let Some(probe) = probe {
                let counters = probe.read();
                probe.open = Some((Instant::now(), counters));
            }
        });
    }

    pub(super) fn end() {
        PROBE.with_borrow_mut(|probe| {
            let Some(probe) = probe else { return };
            let Some((started, start)) = probe.open.take() else {
                return;
            };
            let wall_ns = started.elapsed().as_nanos() as u64;
            let end = probe.read();
            let freq_khz = probe.freq(end.cpu);
            if probe.samples.len() < probe.samples.capacity() {
                probe.samples.push(Sample {
                    wall_ns,
                    start,
                    end,
                    freq_khz,
                });
            } else {
                probe.dropped += 1;
            }
        });
    }

    /// One entry per recorded paint, in paint order, or `None` when not recording.
    pub(super) fn collect() -> Option<Value> {
        PROBE.with_borrow_mut(|probe| {
            let probe = probe.take()?;
            let rows: Vec<Value> = probe
                .samples
                .iter()
                .map(|s| {
                    json!([
                        s.wall_ns,
                        s.end.cpu_ns.saturating_sub(s.start.cpu_ns),
                        s.end.wait_ns.saturating_sub(s.start.wait_ns),
                        s.end.slices.saturating_sub(s.start.slices),
                        s.end.minflt.saturating_sub(s.start.minflt),
                        s.end.majflt.saturating_sub(s.start.majflt),
                        s.end.voluntary.saturating_sub(s.start.voluntary),
                        s.end.involuntary.saturating_sub(s.start.involuntary),
                        s.start.cpu,
                        s.end.cpu,
                        s.freq_khz,
                        hw_delta(s.start.hw[0], s.end.hw[0]),
                        hw_delta(s.start.hw[1], s.end.hw[1]),
                        hw_delta(s.start.hw[2], s.end.hw[2]),
                        s.start.multiplexed || s.end.multiplexed,
                    ])
                })
                .collect();
            Some(json!({
                "columns": ["wall_ns", "cpu_ns", "runqueue_wait_ns", "timeslices", "minor_faults",
                            "major_faults", "voluntary_switches", "involuntary_switches",
                            "cpu_start", "cpu_end", "end_cpu_cur_freq_khz", "user_cycles",
                            "user_instructions", "user_ref_cycles", "counters_multiplexed"],
                "scope": "every SceneElement paint, warm-up included, one per draw",
                "dropped": probe.dropped,
                "rows": rows,
            }))
        })
    }

    impl Probe {
        fn read(&self) -> Counters {
            use std::os::unix::fs::FileExt;
            let mut counters = Counters::default();
            // SAFETY: plain syscalls writing into locals we own.
            unsafe {
                let mut ts = std::mem::zeroed::<libc::timespec>();
                if libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) == 0 {
                    counters.cpu_ns = ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64;
                }
                let mut usage = std::mem::zeroed::<libc::rusage>();
                if libc::getrusage(libc::RUSAGE_THREAD, &mut usage) == 0 {
                    counters.minflt = usage.ru_minflt as u64;
                    counters.majflt = usage.ru_majflt as u64;
                    counters.voluntary = usage.ru_nvcsw as u64;
                    counters.involuntary = usage.ru_nivcsw as u64;
                }
                counters.cpu = libc::sched_getcpu().max(0) as u32;
            }
            // Run-queue wait is accounted when the thread is next scheduled, so a wait that
            // ended inside the paint is already counted when the paint ends.
            let mut buf = [0u8; 128];
            if let Ok(n) = self.files.schedstat.read_at(&mut buf, 0) {
                let mut fields = fields(&buf[..n]);
                let _cpu_ns_at_last_switch = fields.next();
                counters.wait_ns = fields.next().unwrap_or(0);
                counters.slices = fields.next().unwrap_or(0);
            }
            for (slot, file) in counters.hw.iter_mut().zip(&self.files.hw) {
                *slot = u64::MAX;
                let Some(file) = file else { continue };
                let mut words = [0u8; 24];
                // perf fds read the current count on every read(); offsets do not apply.
                if std::io::Read::read(&mut &*file, &mut words).ok() == Some(24) {
                    let word = |i: usize| {
                        u64::from_ne_bytes(words[i * 8..i * 8 + 8].try_into().unwrap_or([0; 8]))
                    };
                    *slot = word(0);
                    counters.multiplexed |= word(1) != word(2);
                }
            }
            counters
        }

        fn freq(&self, cpu: u32) -> u64 {
            use std::os::unix::fs::FileExt;
            let Some(Some(file)) = self.files.freq.get(cpu as usize) else {
                return 0;
            };
            let mut buf = [0u8; 32];
            file.read_at(&mut buf, 0)
                .map_or(0, |n| fields(&buf[..n]).next().unwrap_or(0))
        }
    }

    /// Counter delta, or null when the counter was unavailable.
    fn hw_delta(start: u64, end: u64) -> Option<u64> {
        (start != u64::MAX && end != u64::MAX).then(|| end.saturating_sub(start))
    }

    pub(super) fn number(digits: &[u8]) -> u64 {
        digits
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .fold(0u64, |n, b| {
                n.saturating_mul(10).saturating_add(u64::from(b - b'0'))
            })
    }

    pub(super) fn fields(text: &[u8]) -> impl Iterator<Item = u64> + '_ {
        text.split(|b| b.is_ascii_whitespace())
            .filter(|f| !f.is_empty())
            .map(number)
    }
}

/// Starts recording on this thread if `OMNIA_BENCH_PAINT_PROBE` is set on Linux. Returns
/// whether it is recording.
pub fn enable(capacity: usize) -> bool {
    #[cfg(target_os = "linux")]
    return linux::enable(capacity);
    #[cfg(not(target_os = "linux"))]
    {
        let _ = capacity;
        false
    }
}

pub fn begin() {
    #[cfg(target_os = "linux")]
    linux::begin();
}

pub fn end() {
    #[cfg(target_os = "linux")]
    linux::end();
}

/// One entry per recorded paint, in paint order, or `None` when not recording.
pub fn collect() -> Option<serde_json::Value> {
    #[cfg(target_os = "linux")]
    return linux::collect();
    #[cfg(not(target_os = "linux"))]
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn parses_whitespace_separated_counters() {
        let values: Vec<u64> = linux::fields(b"282884 6440 2\n").collect();
        assert_eq!(values, [282_884, 6_440, 2]);
        assert_eq!(linux::number(b"1399989\n"), 1_399_989);
    }

    #[test]
    fn disabled_probe_records_nothing() {
        begin();
        end();
        assert!(collect().is_none());
    }
}
