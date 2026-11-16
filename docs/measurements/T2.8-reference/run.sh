#!/usr/bin/env bash
set -euo pipefail
cd /home/marcus/t28-reference-13b4809
evidence=docs/measurements/T2.8-reference/raw
mkdir "$evidence"
binary=docs/measurements/T2.8-reference/runner/target/release/t28-reference
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
    sha256sum "$binary" /home/marcus/omnia/target/release/omnia-bench
    git -C /home/marcus/omnia rev-parse HEAD
    git -C /home/marcus/omnia status --short
    find crates/omnia-doc crates/omnia-symbols -type f -print0 | sort -z | xargs -0 sha256sum
    sha256sum crates/omnia-bench/src/generate.rs docs/measurements/T2.8-reference/runner/{main.rs,Cargo.toml,Cargo.lock}
    vulkaninfo --summary
} > "$evidence/machine.txt" 2>&1
cases=(real_10k_leaf label_5k_gnd_vcc_bridge chain_20k_middle index_10k)
for pass in {1..10}; do
    for offset in {0..3}; do
        name=${cases[$(((pass-1+offset)%4))]}
        tag=$(printf '%02d-%s' "$pass" "$name")
        conditions > "$evidence/$tag.before.txt"
        printf '%s %s\n' "$(date -Is)" "$tag" >> "$evidence/order.txt"
        timeout --signal=TERM --kill-after=5s 30s "$binary" "$name" > "$evidence/$tag.csv" 2> "$evidence/$tag.stderr.txt"
        conditions > "$evidence/$tag.after.txt"
    done
done
{
    sha256sum "$binary" /home/marcus/omnia/target/release/omnia-bench
    git -C /home/marcus/omnia rev-parse HEAD
    git -C /home/marcus/omnia status --short
} > "$evidence/preserved.txt"
