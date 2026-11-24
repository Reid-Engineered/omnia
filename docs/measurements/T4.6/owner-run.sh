#!/usr/bin/env bash
# T4.6 Milestone 4 reference campaign on Orion, started by the owner at the desk.
#
# One command, from a clean checkout of this repository on Orion, in the local Wayland session:
#
#     bash docs/measurements/T4.6/owner-run.sh           # the campaign (about 45 minutes)
#     bash docs/measurements/T4.6/owner-run.sh --check   # preflight only; changes nothing
#
# It runs the frozen gate (docs/measurements/T4.6/gate.py) unchanged. Around it, it:
#   1. refuses to start over SSH, or while T3 Code, a coding agent, cargo or rustc is running
#      (docs/measurements/T4.6-stall.md: every slow draw was the bench preempted by them);
#   2. checks out the current origin/main and builds the release bench;
#   3. asks for sudo once, at the start. A root helper then sets the `performance` governor on
#      every CPU, stops tailscaled and (as the owner) suspends Baloo, and puts all three back
#      when this script exits for any reason: success, failure, Ctrl-C or a closed terminal;
#   4. records every process's CPU time just before and just after the gate, so the foreign
#      load over the whole campaign can be read off afterwards (evidence, not a gate input);
#   5. runs the gate under kde-inhibit.
#
# Output goes outside the checkout, because the gate requires a clean tree:
# $HOME/omnia-t46-owner-<UTC time>/ holds gate/ (the gate's own output), cpu-before.json,
# cpu-after.json, conditions.txt, gate-exit.txt and owner-run.log.
# Leave the laptop alone until it prints its verdict.

HELPER_PID=

main() {
    set -euo pipefail
    local root check=0
    root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
    if [[ ${1:-} == --check ]]; then check=1; fi

    local blockers=() supply mains=0 busy tool
    if [[ -n ${SSH_CONNECTION:-}${SSH_TTY:-} ]]; then
        blockers+=("started over SSH: run it at Orion's desk")
    fi
    if [[ ${XDG_SESSION_TYPE:-} != wayland ]]; then
        blockers+=("not in the local Wayland session")
    fi
    if ! grep -q 'i5-7300U' /proc/cpuinfo; then
        blockers+=("not Orion (expected the Core i5-7300U)")
    fi
    for supply in /sys/class/power_supply/*; do
        if [[ $(cat "$supply/type" 2>/dev/null) == Mains && $(cat "$supply/online" 2>/dev/null) == 1 ]]; then
            mains=1
        fi
    done
    if ((mains == 0)); then blockers+=("AC power is not connected"); fi
    busy=$(pgrep -a -i -f '(^|/)(t3code|t3-code|claude|codex|cargo|rustc)( |$)' || true)
    if [[ -n $busy ]]; then
        blockers+=("close these first (T3 Code, coding agents, builds):"$'\n'"$busy")
    fi
    if [[ -n $(git -C "$root" status --porcelain) ]]; then
        blockers+=("the checkout at $root is not clean (untracked files count too)")
    fi
    for tool in kscreen-doctor kde-inhibit balooctl6 python3 cargo sudo runuser; do
        if ! command -v "$tool" >/dev/null; then blockers+=("missing $tool"); fi
    done

    if ((${#blockers[@]})); then
        printf 'owner-run: not ready\n' >&2
        printf '  - %s\n' "${blockers[@]}" >&2
        return 2
    fi
    if ((check)); then
        echo "owner-run: ready (preflight only; nothing changed)"
        return 0
    fi

    local out
    out="$HOME/omnia-t46-owner-$(date -u +%Y%m%dT%H%M%SZ)"
    mkdir "$out"
    exec > >(tee -a "$out/owner-run.log") 2>&1
    echo "owner-run: output $out"

    git -C "$root" fetch --quiet origin main
    git -C "$root" checkout --quiet --detach origin/main
    local commit
    commit=$(git -C "$root" rev-parse HEAD)

    local governors baloo_was tailscale_was
    governors=$(grep -H . /sys/devices/system/cpu/cpu[0-9]*/cpufreq/scaling_governor)
    baloo_was=$(balooctl6 status 2>/dev/null | head -1 || true)
    tailscale_was=$(systemctl is-active tailscaled 2>/dev/null || true)
    {
        echo "commit $commit"
        echo "governors before: $(echo "$governors" | tr '\n' ' ')"
        echo "baloo before: $baloo_was"
        echo "tailscaled before: $tailscale_was"
        echo "kernel $(uname -r); started $(date -u +%FT%TZ)"
    } > "$out/conditions.txt"

    echo "owner-run: sudo is needed once, now, for the governor, tailscaled and the restore"
    sudo -v
    local fifo="$out/.restore"
    mkfifo "$fifo"
    # The root helper blocks on the FIFO without using CPU. When this script exits for any reason
    # the write end closes, `read` returns and the helper restores the recorded state. It ignores
    # HUP, INT and TERM so that it outlives a closed terminal or Ctrl-C.
    sudo env GOVERNORS="$governors" TAILSCALE_WAS="$tailscale_was" BALOO_WAS="$baloo_was" \
        OWNER="$USER" OWNER_BUS="${DBUS_SESSION_BUS_ADDRESS:-}" \
        OWNER_RUNTIME="${XDG_RUNTIME_DIR:-}" FIFO="$fifo" bash -c '
        trap "" HUP INT TERM
        exec 3< "$FIFO"
        as_owner() {
            runuser -u "$OWNER" -- env DBUS_SESSION_BUS_ADDRESS="$OWNER_BUS" \
                XDG_RUNTIME_DIR="$OWNER_RUNTIME" "$@"
        }
        for path in /sys/devices/system/cpu/cpu[0-9]*/cpufreq/scaling_governor; do
            echo performance > "$path"
        done
        if [[ $TAILSCALE_WAS == active ]]; then systemctl stop tailscaled; fi
        as_owner balooctl6 suspend > /dev/null 2>&1
        read -r _ <&3 || true
        while IFS=: read -r path value; do
            printf "%s\n" "$value" > "$path" || true
        done <<< "$GOVERNORS"
        if [[ $TAILSCALE_WAS == active ]]; then systemctl start tailscaled || true; fi
        if [[ $BALOO_WAS != *suspended* ]]; then as_owner balooctl6 resume > /dev/null 2>&1; fi
        echo "owner-run: restored governors, tailscaled ($TAILSCALE_WAS) and Baloo"
    ' &
    HELPER_PID=$!
    exec 9>"$fifo"
    rm -f "$fifo"
    trap release EXIT

    echo "owner-run: building origin/main $commit"
    (cd "$root" && cargo build --locked --release -p omnia-bench)

    echo "owner-run: settling for 60 s after the build"
    sleep 60
    if grep -L -x performance /sys/devices/system/cpu/cpu[0-9]*/cpufreq/scaling_governor | grep -q .; then
        echo "owner-run: the helper did not set the performance governor" >&2
        return 2
    fi
    busy=$(pgrep -a -i -f '(^|/)(t3code|t3-code|claude|codex|cargo|rustc)( |$)' || true)
    if [[ -n $busy ]]; then
        printf 'owner-run: started during the build; close them and run again:\n%s\n' "$busy" >&2
        return 2
    fi

    local status=0
    cpu_times > "$out/cpu-before.json"
    kde-inhibit --power --screenSaver \
        python3 -B "$root/docs/measurements/T4.6/gate.py" run "$out/gate" || status=$?
    cpu_times > "$out/cpu-after.json"
    echo "$status" > "$out/gate-exit.txt"
    echo "finished $(date -u +%FT%TZ), gate exit $status" >> "$out/conditions.txt"

    echo
    echo "owner-run: gate exit $status ($(cat "$out/gate/status.txt" 2>/dev/null || echo 'no status'))"
    echo "owner-run: evidence in $out"
    return "$status"
}

release() {
    exec 9>&-
    if [[ -n $HELPER_PID ]]; then wait "$HELPER_PID" || true; fi
}

cpu_times() {
    python3 -I -c '
import json, pathlib
out = {}
for stat in pathlib.Path("/proc").glob("[0-9]*/stat"):
    try:
        text = stat.read_text()
    except OSError:
        continue
    comm = text[text.index("(") + 1:text.rindex(")")]
    fields = text[text.rindex(")") + 2:].split()
    out[stat.parent.name] = [comm, int(fields[11]) + int(fields[12])]
print(json.dumps(out, sort_keys=True))
'
}

# Kept on one line: bash reads a whole line before running it, so the checkout inside `main`
# cannot change what runs next if it replaces this file on disk.
main "$@"; exit $?
