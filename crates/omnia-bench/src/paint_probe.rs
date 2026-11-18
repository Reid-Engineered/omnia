//! Diagnostic only (T2.7 paint-stall investigation): per-paint thread scheduling counters.
//!
//! With `OMNIA_BENCH_PAINT_PROBE=1` on Linux, every `SceneElement` paint records, from
//! `/proc/thread-self`: wall time, thread CPU time, run-queue wait and timeslices
//! (`schedstat`), minor and major page faults and the CPU the thread was on (`stat`), and the
//! end CPU's `scaling_cur_freq`. Wall time well above CPU time means the thread was not
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
        cpu: u32,
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
        stat: std::fs::File,
        freq: Vec<Option<std::fs::File>>,
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
            let (Ok(schedstat), Ok(stat)) = (
                File::open("/proc/thread-self/schedstat"),
                File::open("/proc/thread-self/stat"),
            ) else {
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
                        stat,
                        freq,
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
                        s.start.cpu,
                        s.end.cpu,
                        s.freq_khz,
                    ])
                })
                .collect();
            Some(json!({
                "columns": ["wall_ns", "cpu_ns", "runqueue_wait_ns", "timeslices", "minor_faults",
                            "major_faults", "cpu_start", "cpu_end", "end_cpu_cur_freq_khz"],
                "scope": "every SceneElement paint, warm-up included, one per draw",
                "dropped": probe.dropped,
                "rows": rows,
            }))
        })
    }

    impl Probe {
        fn read(&self) -> Counters {
            use std::os::unix::fs::FileExt;
            let mut buf = [0u8; 1024];
            let mut counters = Counters::default();
            if let Ok(n) = self.files.schedstat.read_at(&mut buf, 0) {
                let mut fields = fields(&buf[..n]);
                counters.cpu_ns = fields.next().unwrap_or(0);
                counters.wait_ns = fields.next().unwrap_or(0);
                counters.slices = fields.next().unwrap_or(0);
            }
            if let Ok(n) = self.files.stat.read_at(&mut buf, 0) {
                // Fields after the `)` closing comm start at field 3 (state).
                let text = &buf[..n];
                if let Some(close) = text.iter().rposition(|&b| b == b')') {
                    let rest = &text[close + 1..];
                    for (index, value) in rest
                        .split(|b| *b == b' ')
                        .filter(|f| !f.is_empty())
                        .enumerate()
                    {
                        let field = index + 3;
                        match field {
                            10 => counters.minflt = number(value),
                            12 => counters.majflt = number(value),
                            39 => {
                                counters.cpu = number(value) as u32;
                                break;
                            }
                            _ => {}
                        }
                    }
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
