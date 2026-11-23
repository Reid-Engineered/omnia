#!/usr/bin/env bash
# One-command fixed ten-pass Orion campaign for W2.2 / WB3 (Move of 100 parts with follow on 10k).
# Follows canvas-plan.md section 2.1 (CPU-only run rules, owner decision 2026-11-17):
#   - reference hardware (Orion, Linux), AC power, `performance` governor
#   - other user apps closed, before/after condition snapshots
#   - 10 interleaved rotating repetitions over the frozen case list
#   - frozen binary and source recorded before and verified after
set -euo pipefail
cd "$(dirname "$0")/../../.."

phase=Preflight
fail() { printf '%s refused: %s\n' "$phase" "$*" >&2; exit 1; }

# All refusal checks precede creation of any evidence directory.
for tool in date cat ps uname lscpu free rustc cargo sha256sum git find sort xargs timeout python3 tar mktemp mkdir; do
    command -v "$tool" >/dev/null || fail "missing tool: $tool"
done
[[ $(uname -s) == Linux ]] || fail 'requires Linux on Orion'

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

# Resolve and freeze the release bench binary before creating any attempt directory.
if [[ -n ${WB3_BIN:-} ]]; then
    [[ -x $WB3_BIN ]] || fail "WB3_BIN is not executable: $WB3_BIN"
    binary=$WB3_BIN
else
    build_json=$(cargo bench -p omnia-bench --bench follow_command --no-run --locked --offline --message-format=json 2>/dev/null) ||
        build_json=$(cargo bench -p omnia-bench --bench follow_command --no-run --locked --message-format=json) ||
        fail 'failed to build omnia-bench follow_command bench'
    binary=$(printf '%s\n' "$build_json" | python3 -c '
import json, sys
paths = []
for line in sys.stdin:
    line = line.strip()
    if not line.startswith("{"):
        continue
    msg = json.loads(line)
    if msg.get("reason") == "compiler-artifact" and msg.get("target", {}).get("name") == "follow_command":
        exe = msg.get("executable")
        if exe:
            paths.append(exe)
assert len(paths) == 1, f"expected 1 follow_command executable, got {paths}"
print(paths[0])
')
    [[ -x $binary ]] || fail "built binary is not executable: $binary"
fi

base=docs/measurements/W2.2
attempt=$(mktemp -d "$base/attempt-XXXXXXXX")
evidence=$attempt/raw
complete=0
phase=Campaign
trap 'if (( complete == 0 )); then printf "Campaign incomplete; retained in %s\n" "$attempt" >&2; fi' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

mkdir -p "$evidence" "$attempt/criterion"
printf 'incomplete\n' > "$attempt/status.txt"

conditions() {
    date -Is
    for supply in "${supplies[@]}"; do
        [[ ! -r $supply/uevent ]] || cat "$supply/uevent"
    done
    cat "${governors[@]}"
    if command -v powerprofilesctl >/dev/null; then
        powerprofilesctl get
    else
        printf 'power profile: unavailable\n'
    fi
    cat /proc/loadavg
    ps -eo pid,ppid,comm,pcpu,args --sort=-pcpu
}

bin_sha_before=$(sha256sum "$binary")
head_before=$(git rev-parse HEAD)
{
    uname -a
    lscpu
    free -h
    cat /sys/class/dmi/id/product_name 2>/dev/null || true
    cat /etc/os-release
    rustc -Vv
    cargo -V
    printf '%s\n' "$bin_sha_before"
    printf 'HEAD=%s\n' "$head_before"
    git status --short -- . ':!docs/measurements/W2.2/attempt-*'
    find crates/omnia-doc crates/omnia-symbols -type f -print0 | sort -z | xargs -0 sha256sum
    sha256sum \
        crates/omnia-bench/Cargo.toml \
        crates/omnia-bench/src/generate.rs \
        crates/omnia-bench/benches/follow_command.rs \
        docs/measurements/W2.2/run.sh \
        docs/measurements/W2.2/summarize.py
    if command -v vulkaninfo >/dev/null; then
        vulkaninfo --summary 2>/dev/null || true
    fi
} > "$evidence/machine.txt" 2>&1

# Frozen case list: the single WB3 bench in crates/omnia-bench/benches/follow_command.rs.
cases=(follow_move_100_parts_10k_apply)
for pass in {1..10}; do
    for offset in "${!cases[@]}"; do
        name=${cases[$(((pass - 1 + offset) % ${#cases[@]}))]}
        tag=$(printf '%02d-%s' "$pass" "$name")
        check_power
        conditions > "$evidence/$tag.before.txt"
        printf '%s %s\n' "$(date -Is)" "$tag" >> "$evidence/order.txt"
        result=0
        OMNIA_WB3_CSV="$evidence/$tag.csv" \
        CRITERION_HOME="$attempt/criterion" \
            timeout --signal=TERM --kill-after=5s 180s \
            "$binary" --bench --exact "$name" --noplot \
            > "$evidence/$tag.stdout.txt" 2> "$evidence/$tag.stderr.txt" || result=$?
        conditions > "$evidence/$tag.after.txt"
        (( result == 0 )) || {
            printf "Invocation failed (%s): %s; incomplete campaign\n" "$result" "$tag" >&2
            exit "$result"
        }
        check_power
    done
done

bin_sha_after=$(sha256sum "$binary")
head_after=$(git rev-parse HEAD)
{
    printf '%s\n' "$bin_sha_after"
    printf 'HEAD=%s\n' "$head_after"
    git status --short -- . ':!docs/measurements/W2.2/attempt-*'
} > "$evidence/preserved.txt"
[[ $bin_sha_before == "$bin_sha_after" ]] || fail 'binary sha256 changed during campaign'
[[ $head_before == "$head_after" ]] || fail 'git HEAD changed during campaign'

tar -czf "$attempt/raw.tar.gz" -C "$attempt" raw
python3 "$base/summarize.py" "$attempt/raw.tar.gz" > "$attempt/summary.txt"
printf 'complete (conditions require separate human review)\n' > "$attempt/status.txt"
complete=1
trap - EXIT INT TERM

cat "$attempt/summary.txt"
printf 'Campaign retained in %s\n' "$attempt"
