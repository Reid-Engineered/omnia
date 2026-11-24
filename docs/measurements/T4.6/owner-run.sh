#!/usr/bin/env bash
# T4.6 Milestone 4 reference campaign on Orion, started by the owner at the desk.
#
# One command, from a clean checkout of this repository on Orion, in a terminal in the local
# Wayland session:
#
#     bash docs/measurements/T4.6/owner-run.sh --check   # preflight only; changes nothing
#     bash docs/measurements/T4.6/owner-run.sh           # the campaign (about 45 minutes)
#
# It runs the frozen gate (docs/measurements/T4.6/gate.py) unchanged. Around it, it:
#   1. refuses to start over SSH, or while T3 Code, a coding agent, cargo or rustc is running
#      (docs/measurements/T4.6-stall.md: every slow draw was the bench preempted by them);
#   2. checks out the current origin/main and builds the release bench;
#   3. asks for sudo once. A root helper sets the `performance` governor on every CPU and stops
#      tailscaled, reports back before anything is timed, and restores both when this script's
#      process ends, however it ends (it waits on a pidfd, so it uses no CPU meanwhile);
#   4. leaves Baloo and other desktop services as they are (canvas plan 2.1: recorded, not
#      stopped), but waits for the indexer to be idle after the build before timing starts;
#   5. records every process's CPU time just before and just after the gate, so the foreign
#      load over the whole campaign can be read off afterwards (evidence, not a gate input);
#   6. runs the gate under kde-inhibit and keeps the gate's own exit status.
#
# Closing the terminal does not stop the campaign (hangups are ignored); Ctrl-C stops it, and
# the gate then restores the panel scale itself. Output goes outside the checkout, because the
# gate requires a clean tree: $HOME/omnia-t46-owner-<UTC time>/ holds gate/ (the gate's own
# output), cpu-before.json, cpu-after.json, conditions.txt, helper.log, gate-exit.txt and
# owner-run.log. Leave the laptop alone until it prints its verdict.

main() {
    set -euo pipefail
    trap '' HUP
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
    busy=$(busy_processes)
    if [[ -n $busy ]]; then
        blockers+=("close these first (T3 Code, coding agents, builds):"$'\n'"$busy")
    fi
    if [[ -n $(git -C "$root" status --porcelain) ]]; then
        blockers+=("the checkout at $root is not clean (untracked files count too)")
    fi
    for tool in kscreen-doctor kde-inhibit qdbus6 balooctl6 python3 cargo sudo tee; do
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
    # tee ignores Ctrl-C (-i) and hangups (inherited), and keeps writing the log file if the
    # terminal goes away.
    exec > >(exec tee -i --output-error=warn -a "$out/owner-run.log") 2>&1
    echo "owner-run: output $out"

    git -C "$root" fetch --quiet origin main
    git -C "$root" checkout --quiet --detach origin/main
    local commit
    commit=$(git -C "$root" rev-parse HEAD)

    local governors tailscale_was
    governors=$(grep -H . /sys/devices/system/cpu/cpu[0-9]*/cpufreq/scaling_governor)
    tailscale_was=$(systemctl is-active tailscaled 2>/dev/null || true)
    {
        echo "commit $commit"
        echo "governors before: $(echo "$governors" | tr '\n' ' ')"
        echo "tailscaled before: $tailscale_was"
        echo "kernel $(uname -r); started $(date -u +%FT%TZ)"
        echo "baloo before:"
        balooctl6 status 2>&1 || true
    } > "$out/conditions.txt"

    echo "owner-run: sudo is needed once, now, for the governor and tailscaled"
    sudo -v
    : > "$out/helper.log"
    # The helper restores the recorded state when this shell's process ends, for any reason.
    # A setup or verification failure restores at once and reports `failed`.
    sudo -n env GOVERNORS="$governors" TAILSCALE_WAS="$tailscale_was" WRAPPER="$$" bash -c '
        trap "" HUP INT TERM
        restore() {
            while IFS=: read -r path value; do
                printf "%s\n" "$value" > "$path" || true
            done <<< "$GOVERNORS"
            if [[ $TAILSCALE_WAS == active ]]; then systemctl start tailscaled || true; fi
            echo "restored governors and tailscaled ($TAILSCALE_WAS) $(date -u +%FT%TZ)"
        }
        for path in /sys/devices/system/cpu/cpu[0-9]*/cpufreq/scaling_governor; do
            echo performance > "$path"
        done
        if [[ $TAILSCALE_WAS == active ]]; then systemctl stop tailscaled; fi
        if grep -L -x performance /sys/devices/system/cpu/cpu[0-9]*/cpufreq/scaling_governor | grep -q . \
            || [[ $TAILSCALE_WAS == active && $(systemctl is-active tailscaled) == active ]]; then
            echo failed; restore; exit 1
        fi
        echo ready
        python3 -I -c "
import os, select, sys
try:
    select.select([os.pidfd_open(int(sys.argv[1]))], [], [])
except ProcessLookupError:
    pass
" "$WRAPPER"
        restore
    ' >> "$out/helper.log" 2>&1 &
    local helper=$! waited=0
    until grep -q -x -e ready -e failed "$out/helper.log"; do
        if ! kill -0 "$helper" 2>/dev/null || ((waited >= 30)); then break; fi
        sleep 1
        waited=$((waited + 1))
    done
    if ! grep -q -x ready "$out/helper.log"; then
        echo "owner-run: the root helper did not get ready; see $out/helper.log" >&2
        return 2
    fi

    echo "owner-run: building origin/main $commit"
    (cd "$root" && cargo build --locked --release -p omnia-bench)

    echo "owner-run: settling for 60 s, then waiting (up to 15 minutes) for Baloo to be idle"
    sleep 60
    waited=0
    until baloo_idle; do
        if ((waited >= 900)); then
            echo "owner-run: Baloo is still indexing after 15 minutes; run again later" >&2
            return 2
        fi
        sleep 10
        waited=$((waited + 10))
    done
    if grep -L -x performance /sys/devices/system/cpu/cpu[0-9]*/cpufreq/scaling_governor | grep -q .; then
        echo "owner-run: the performance governor is not set on every CPU" >&2
        return 2
    fi
    busy=$(busy_processes)
    if [[ -n $busy ]]; then
        printf 'owner-run: started during the build; close them and run again:\n%s\n' "$busy" >&2
        return 2
    fi

    cpu_times > "$out/cpu-before.json"
    # kde-inhibit always exits 0, so the gate's status is written from inside it. Whether the
    # inhibitors registered is recorded; the gate itself checks DPMS before and after every run.
    kde-inhibit --power --screenSaver bash -c '
        qdbus6 org.freedesktop.PowerManagement /org/freedesktop/PowerManagement/Inhibit \
            HasInhibit > "$1/inhibit.txt" 2>&1 || true
        python3 -B "$2" run "$1/gate"
        echo $? > "$1/gate-exit.txt"
    ' owner-run "$out" "$root/docs/measurements/T4.6/gate.py" || true
    cpu_times > "$out/cpu-after.json"
    local status
    status=$(cat "$out/gate-exit.txt" 2>/dev/null || echo 2)
    {
        echo "finished $(date -u +%FT%TZ), gate exit $status"
        echo "power-management inhibit during the gate: $(cat "$out/inhibit.txt" 2>/dev/null)"
        echo "baloo after:"
        balooctl6 status 2>&1 || true
    } >> "$out/conditions.txt"

    echo
    echo "owner-run: gate exit $status ($(cat "$out/gate/status.txt" 2>/dev/null || echo 'no status'))"
    echo "owner-run: evidence in $out; the helper restores the governor and tailscaled on exit"
    return "$status"
}

baloo_idle() {
    local status
    status=$(balooctl6 status 2>&1) || return 0
    [[ $status != *"is running"* ]] && return 0
    grep -q -x 'Indexer state: Idle' <<< "$status" \
        && ! grep -q -E '^Files waiting for content indexing: [1-9]' <<< "$status"
}

# Exact process names, executable names and path components of argv entries, so that an
# Electron app (/opt/t3code/...), a node-launched CLI (.../codex.js, .../claude-code/cli.js) or
# a cargo build is found, and a shell command that merely mentions one of them is not.
busy_processes() {
    python3 -I -c '
import os
names = {"t3code", "t3-code", "claude", "claude-code", "codex", "cargo", "rustc"}
me = os.getpid()
for pid in sorted((p for p in os.listdir("/proc") if p.isdigit()), key=int):
    if int(pid) == me:
        continue
    try:
        with open(f"/proc/{pid}/comm") as f:
            comm = f.read().strip()
        with open(f"/proc/{pid}/cmdline", "rb") as f:
            argv = [a.decode(errors="replace") for a in f.read().split(b"\0") if a]
    except OSError:
        continue
    try:
        exe = os.path.basename(os.readlink(f"/proc/{pid}/exe"))
    except OSError:
        exe = ""
    seen = {comm.lower(), exe.lower()}
    for arg in argv:
        if any(c.isspace() for c in arg):
            continue
        for part in arg.lower().split("/"):
            seen.add(part)
            for ext in (".js", ".cjs", ".mjs", ".asar", ".exe"):
                if part.endswith(ext):
                    seen.add(part[: -len(ext)])
    if seen & names:
        print(pid, comm, " ".join(argv)[:160])
'
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
