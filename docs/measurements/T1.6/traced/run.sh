#!/usr/bin/env bash
# T1.6 / audit 4.1: one bounded diagnostic campaign, never a gate verdict.
set -euo pipefail
cd "$(dirname "$0")/../../../.."
base=docs/measurements/T1.6/traced
bench=target/release/omnia-bench
fail() { printf 'Refused: %s\n' "$*" >&2; exit 1; }
# Preflight is read-only. No evidence directory exists until all checks pass.
for tool in uname cat ps date git sha256sum python3 vulkaninfo kscreen-doctor timeout mktemp mkdir cp kde-inhibit; do
    command -v "$tool" >/dev/null || fail "missing tool: $tool"
done
[[ $(uname -s) == Linux ]] || fail 'requires Linux on Orion'
[[ ${XDG_SESSION_TYPE:-} == wayland && -n ${WAYLAND_DISPLAY:-} ]] || fail 'requires the local Wayland session'
# kde-inhibit starts the --inhibited pass with stdin detached from the terminal;
# reattach the controlling terminal so the scale-switch prompt can still be answered.
if [[ ${1:-} == --inhibited ]]; then exec </dev/tty || fail 'controlling terminal unavailable after kde-inhibit'; fi
[[ -t 0 ]] || fail 'requires owner at an interactive terminal for the scale switch'
[[ -x $bench ]] || fail 'build first: cargo build --release --locked -p omnia-bench'
[[ -z $(git status --porcelain --untracked-files=no) ]] || fail 'tracked source must be clean'
python3 "$base/conditions.py" check 1
vulkaninfo --summary >/dev/null 2>&1 || fail 'Vulkan inventory unavailable'
# Inhibit sleep only after read-only preflight, scoped to this single campaign.
if [[ ${1:-} != --inhibited ]]; then
    for prior in "$base"/attempt-*; do
        if [[ -d $prior ]]; then
            printf 'NOTICE: prior attempt exists (%s); policy forbids a second campaign without owner direction.\n' "$prior"
            break
        fi
    done
    printf 'Close other user apps, keep the bench foreground, stay on AC.\n'
    read -r -p 'At Orion, apps closed, release binary built from this clean source? Type RUN: ' answer
    [[ $answer == RUN ]] || fail 'owner not ready'
    exec kde-inhibit --power --screenSaver bash "$base/run.sh" --inhibited
fi
attempt=$(mktemp -d "$base/attempt-XXXXXXXX")
complete=0
finish() {
    local code=$?
    trap - EXIT
    python3 "$base/conditions.py" snapshot > "$attempt/after.json" || true
    sha256sum "$bench" > "$attempt/binary-after.sha256" || true
    if (( complete == 0 )); then
        printf 'incomplete (exit %s); diagnostic, not a gate verdict\n' "$code" > "$attempt/status.txt"
    fi
    printf 'Retained %s; panel scale left at current setting.\n' "$attempt"
    exit "$code"
}
trap finish EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
run_analysis() {
    local input=$1 output=$2 analysis_code=0
    python3 "$base/analyze.py" "$input" > "$output" 2>&1 || analysis_code=$?
    if (( analysis_code != 0 )); then
        printf 'analysis-error: %s (exit %s); campaign continues\n' "$input" "$analysis_code" >> "$attempt/analysis-errors.txt"
        printf 'analysis-error: %s (exit %s); campaign continues\n' "$input" "$analysis_code"
    fi
}
printf 'incomplete; diagnostic, not a gate verdict\n' > "$attempt/status.txt"
cp "$base/run.sh" "$base/conditions.py" "$base/analyze.py" docs/measurements/T1.6/summarize.py "$attempt/"
git rev-parse HEAD > "$attempt/source-commit.txt"
sha256sum "$bench" > "$attempt/binary.sha256"
binary_hash=$(sha256sum "$bench")
source_commit=$(git rev-parse HEAD)
{
    uname -a
    cat /etc/os-release /proc/cpuinfo /proc/meminfo
    vulkaninfo --summary
} > "$attempt/hardware.txt" 2>&1
python3 "$base/conditions.py" snapshot > "$attempt/before.json"
cases=(start pan zoom idle)
# Freeze the entire order before the first invocation.
for tag in s100 s125; do
    for r in {1..10}; do
        for j in {0..3}; do
            printf '%s %s-%s\n' "$tag" "${cases[$(((j+r-1)%4))]}" "$r"
        done
    done
done > "$attempt/order.txt"
for tag in s100 s125; do
    if [[ $tag == s100 ]]; then scale=1; size=1920x1080; else
        scale=1.25; size=1536x864
        printf '100%% leg finished. Set the native eDP-1 panel to 125%% in display settings; close settings.\n'
        read -r -p 'Return here, then press Enter to VERIFY 125% and continue (Ctrl-C aborts): ' answer
    fi
    python3 "$base/conditions.py" check "$scale"
    dir=$attempt/$tag
    mkdir "$dir"
    for r in {1..10}; do
        for j in {0..3}; do
            c=${cases[$(((j+r-1)%4))]}
            key=$c-$r
            json=$dir/$key.json
            python3 "$base/conditions.py" record "$dir" "$key" "$r" before null > "$dir/$key.before.json"
            python3 "$base/conditions.py" check "$scale"
            [[ $(sha256sum "$bench") == "$binary_hash" && $(git rev-parse HEAD) == "$source_commit" ]] || fail 'binary/source changed; campaign incomplete'
            [[ -z $(git status --porcelain --untracked-files=no) ]] || fail 'tracked source changed'
            case $c in
                start) args=(start --sheet empty --runs 10) ;;
                pan|zoom) args=(run --sheet empty --script "$c" --frames 600 --warmup 60) ;;
                idle) args=(idle --sheet empty --seconds 10) ;;
            esac
            printf '%s %s\n' "$tag" "$key"
            code=0
            OMNIA_BENCH_FULLSCREEN=1 OMNIA_BENCH_TRACE=1 WAYLAND_DEBUG=client \
                timeout --foreground --signal=TERM --kill-after=15s 120s "$bench" "${args[@]}" --size "$size" --json "$json" \
                > "$dir/$key.stdout.log" 2> "$dir/$key.wayland.log" || code=$?
            printf '%s\n' "$code" > "$dir/$key.exit.txt"
            python3 "$base/conditions.py" record "$dir" "$key" "$r" after "$code" > "$dir/$key.after.json"
            (( code == 0 )) || fail "$tag/$key exited $code; no retries"
            python3 "$base/conditions.py" check "$scale"
            python3 "$base/conditions.py" report "$json" "$scale"
            if [[ $c == pan || $c == zoom ]]; then
                run_analysis "$json" "$dir/$key.analysis.txt"
            fi
        done
    done
done
[[ $(sha256sum "$bench") == "$binary_hash" && $(git rev-parse HEAD) == "$source_commit" ]] || fail 'binary/source changed'
[[ -z $(git status --porcelain --untracked-files=no) ]] || fail 'tracked source changed'
# The copied gate summarizer resolves s100/s125 relative to itself. Only now,
# with all 80 reports present, run it; its verdict is diagnostic under tracing.
summary_code=0
python3 "$attempt/summarize.py" > "$attempt/gate-summary-diagnostic.txt" 2>&1 || summary_code=$?
printf 'Gate summarizer exit %s; traced data is diagnostic only.\n' "$summary_code" >> "$attempt/gate-summary-diagnostic.txt"
(( summary_code <= 1 )) || fail 'summarizer crashed'
printf 'complete\n' > "$attempt/status.txt"
complete=1
run_analysis "$attempt" "$attempt/analysis.txt"
printf '\nCampaign finished (diagnostic only, not a gate verdict). Result:\n'
tail -n 3 "$attempt/analysis.txt"
if [[ -s "$attempt/analysis-errors.txt" ]]; then printf 'Analysis errors recorded: %s\n' "$attempt/analysis-errors.txt"; fi
printf 'Return the whole attempt directory: %s\n' "$attempt"
