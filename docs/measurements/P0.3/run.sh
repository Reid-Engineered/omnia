#!/usr/bin/env bash
# P0.3 / PS1 one-command run on Linux (for an owner-run Orion reference pass of the PB3 index
# timing). Same schedule and binaries as run.ps1. A run here records numbers; whether they
# certify PB3 is decided by P1.5 / P3.1 (HP1), not by this spike.
#
#   bash docs/measurements/P0.3/run.sh [label]      # default label: linux
set -euo pipefail

# Schedule, fixed before the first timed run.
PARTS=500 REPS=10 ITERS=200 WARMUP=20

label="${1:-linux}"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
results="$here/results"
mkdir -p "$results"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$here/spike/target}"
bin="$CARGO_TARGET_DIR/release"

(cd "$here/spike" && cargo build --offline --locked --release --bins)
(cd "$bin" && sha256sum extract index_bench) > "$results/binaries-$label.sha256"

snapshot() {
  echo "timestamp: $(date -Is)"
  echo "kernel: $(uname -srmo)"
  [ -r /etc/os-release ] && echo "os: $(. /etc/os-release && echo "$PRETTY_NAME")"
  echo "cpu: $(lscpu | sed -n 's/^Model name: *//p')"
  echo "governor: $(cat /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor 2>/dev/null | sort | uniq -c | tr -s ' ' | tr '\n' ';')"
  for s in /sys/class/power_supply/*; do
    [ -r "$s/type" ] || continue
    echo "power_supply $(basename "$s"): type=$(cat "$s/type") online=$(cat "$s/online" 2>/dev/null || echo -) status=$(cat "$s/status" 2>/dev/null || echo -)"
  done
  echo "power_profile: $(powerprofilesctl get 2>/dev/null || echo unknown)"
  echo "memory: $(free -m | awk '/^Mem:/ {print "total_mb="$2" free_mb="$4}')"
  echo "load: $(cat /proc/loadavg)"
  echo "top_cpu_processes (name = first 8 hex digits of SHA-256(name)):"
  ps -eo comm=,pcpu=,rss= --sort=-pcpu | head -8 | while read -r c p r; do
    echo "  proc-$(printf '%s' "$c" | sha256sum | cut -c1-8) pcpu=$p rss_kb=$r"
  done
  echo "rustc: $(rustc --version)"
}

snapshot > "$results/conditions-before-$label.txt"
"$bin/extract" "$here"
"$bin/index_bench" --work "$CARGO_TARGET_DIR/p03-work" --out "$results/index-$label" \
  --parts "$PARTS" --reps "$REPS" --iters "$ITERS" --warmup "$WARMUP" | tee "$results/index-$label.txt"
snapshot > "$results/conditions-after-$label.txt"
