#!/usr/bin/env bash
# Fixed ten-pass rotating campaign for the command-boundary cases.
# Run on the reference machine (Orion, Linux) under canvas-plan section 2.1;
# see docs/measurements/command-boundary.md. Freeze the case list, source and
# binary before the first timed run; do not retry until green or drop cases.
set -euo pipefail
cd "$(dirname "$0")/../../.."
runner=docs/measurements/command-boundary/runner
binary=$runner/target/release/command-boundary
fail() { printf 'Preflight refused: %s\n' "$*" >&2; exit 1; }
# All refusal checks precede creation of any evidence directory.
for tool in date cat ps uname lscpu free rustc cargo sha256sum git find sort xargs vulkaninfo timeout python3 tar mktemp mkdir; do
    command -v "$tool" >/dev/null || fail "missing tool: $tool"
done
[[ $(uname -s) == Linux ]] || fail 'requires Linux on Orion'
[[ -x $binary ]] || fail "build runner first: cargo build --manifest-path $runner/Cargo.toml --release --offline -j 2"
shopt -s nullglob
supplies=(/sys/class/power_supply/*)
governors=(/sys/devices/system/cpu/cpu*/cpufreq/scaling_governor)
check_power() {
    local supply ac=0 governor
    for supply in "${supplies[@]}"; do
        if [[ -r $supply/type && -r $supply/online ]] &&
           [[ $(cat "$supply/type") == Mains && $(cat "$supply/online") == 1 ]]; then
            ac=1
        fi
    done
    (( ac == 1 )) || fail 'AC power could not be verified'
    (( ${#governors[@]} > 0 )) || fail 'CPU governor files unavailable'
    for governor in "${governors[@]}"; do
        [[ $(cat "$governor") == performance ]] || fail "performance governor required: $governor"
    done
}
check_power
# Verify current public APIs at freeze; no GPUI-heavy benchmark binary is used.
cargo check --manifest-path "$runner/Cargo.toml" --offline
vulkaninfo --summary >/dev/null 2>&1 || fail 'Vulkan inventory unavailable'
# Each invocation preserves a distinct attempt, including incomplete campaigns.
base=docs/measurements/command-boundary
attempt=$(mktemp -d "$base/attempt-XXXXXXXX")
evidence=$attempt/raw
mkdir "$evidence"
printf 'incomplete\n' > "$attempt/status.txt"
complete=0
trap 'if (( complete == 0 )); then printf "Campaign incomplete; retained in %s\n" "$attempt" >&2; fi' EXIT
trap 'printf "Campaign interrupted; retained in %s\n" "$attempt" >&2; exit 130' INT TERM
conditions() {
    date -Is
    for supply in "${supplies[@]}"; do
        [[ ! -r $supply/uevent ]] || cat "$supply/uevent"
    done
    cat "${governors[@]}"
    if command -v powerprofilesctl >/dev/null; then powerprofilesctl get; else printf 'power profile: unavailable\n'; fi
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
    sha256sum "$binary"
    git rev-parse HEAD
    git status --short -- . ':!docs/measurements/command-boundary/attempt-*'
    find crates/omnia-doc crates/omnia-symbols -type f -print0 | sort -z | xargs -0 sha256sum
    sha256sum crates/omnia-bench/src/generate.rs docs/measurements/command-boundary/runner/main.rs docs/measurements/command-boundary/runner/Cargo.toml docs/measurements/command-boundary/runner/Cargo.lock docs/measurements/command-boundary/run.sh docs/measurements/command-boundary/summarize.py
    vulkaninfo --summary
} > "$evidence/machine.txt" 2>&1
# Frozen case list; mirrors the match arms in runner/main.rs.
cases=(real_10k_leaf label_5k_gnd_vcc_bridge chain_20k_middle index_10k doc_10k_wire_add doc_10k_wire_delete doc_10k_move_100 doc_10k_move_100_connected doc_chain_20k_add doc_chain_20k_delete doc_label_bridge_add doc_label_bridge_delete)
for pass in {1..10}; do
    for offset in "${!cases[@]}"; do
        name=${cases[$(((pass-1+offset)%${#cases[@]}))]}
        tag=$(printf '%02d-%s' "$pass" "$name")
        check_power
        conditions > "$evidence/$tag.before.txt"
        printf '%s %s\n' "$(date -Is)" "$tag" >> "$evidence/order.txt"
        result=0
        timeout --signal=TERM --kill-after=5s 30s "$binary" "$name" > "$evidence/$tag.csv" 2> "$evidence/$tag.stderr.txt" || result=$?
        conditions > "$evidence/$tag.after.txt"
        (( result == 0 )) || { printf "Invocation failed (%s): %s; incomplete campaign\n" "$result" "$tag" >&2; exit "$result"; }
        check_power
    done
done
{
    sha256sum "$binary"
    git rev-parse HEAD
    git status --short -- . ':!docs/measurements/command-boundary/attempt-*'
} > "$evidence/preserved.txt"

tar -czf "$attempt/raw.tar.gz" -C "$attempt" raw
python3 "$base/summarize.py" "$attempt/raw.tar.gz" > "$attempt/summary.txt"
printf 'complete (conditions require separate human review)\n' > "$attempt/status.txt"
complete=1
trap - EXIT INT TERM
printf 'Campaign retained in %s\n' "$attempt"
