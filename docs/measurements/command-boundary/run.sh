#!/usr/bin/env bash
# Fixed ten-pass rotating campaign for the command-boundary cases.
# Run on the reference machine (Orion, Linux) under canvas-plan section 2.1;
# see docs/measurements/command-boundary.md. Freeze the case list, source and
# binary before the first timed run; do not retry until green or drop cases.
set -euo pipefail
cd "$(dirname "$0")/../../.."
evidence=docs/measurements/command-boundary/raw
mkdir "$evidence"
binary=docs/measurements/command-boundary/runner/target/release/command-boundary
conditions() {
    date -Is
    cat /sys/class/power_supply/AC/online
    cat /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor
    cat /proc/loadavg
    ps -eo pid,ppid,comm,pcpu,args --sort=-pcpu
}
{
    uname -a
    lscpu
    free -h
    cat /sys/class/dmi/id/product_name
    cat /etc/os-release
    rustc -Vv
    cargo -V
    sha256sum "$binary" target/release/omnia-bench
    git rev-parse HEAD
    git status --short
    find crates/omnia-doc crates/omnia-symbols -type f -print0 | sort -z | xargs -0 sha256sum
    sha256sum crates/omnia-bench/src/generate.rs docs/measurements/command-boundary/runner/main.rs docs/measurements/command-boundary/runner/Cargo.toml docs/measurements/command-boundary/runner/Cargo.lock
    vulkaninfo --summary
} > "$evidence/machine.txt" 2>&1
# Frozen case list; mirrors the match arms in runner/main.rs.
cases=(real_10k_leaf label_5k_gnd_vcc_bridge chain_20k_middle index_10k doc_10k_wire_add doc_10k_wire_delete doc_10k_move_100 doc_chain_20k_add doc_chain_20k_delete doc_label_bridge_add doc_label_bridge_delete)
for pass in {1..10}; do
    for offset in "${!cases[@]}"; do
        name=${cases[$(((pass-1+offset)%${#cases[@]}))]}
        tag=$(printf '%02d-%s' "$pass" "$name")
        conditions > "$evidence/$tag.before.txt"
        printf '%s %s\n' "$(date -Is)" "$tag" >> "$evidence/order.txt"
        timeout --signal=TERM --kill-after=5s 30s "$binary" "$name" > "$evidence/$tag.csv" 2> "$evidence/$tag.stderr.txt"
        conditions > "$evidence/$tag.after.txt"
    done
done
{
    sha256sum "$binary" target/release/omnia-bench
    git rev-parse HEAD
    git status --short
} > "$evidence/preserved.txt"
