#!/usr/bin/env bash
# T7.5 final canvas budget gate (B1 to B10) on Orion, started by the owner at the desk.
#
# One command, from a clean checkout of this repository on Orion, in a terminal in the local
# Wayland session:
#
#     bash docs/measurements/T7.5/owner-run.sh --check   # preflight only; changes nothing
#     bash docs/measurements/T7.5/owner-run.sh           # the campaign (B1-B10, ~65-70 min)
#
# Remote start (requires fresh owner approval for T7.5, after confirming the lid is open, the
# panel on and unlocked, AC connected and nobody using the laptop):
#
#     umask 077; cat > /run/user/$UID/t75-pw
#     systemd-run --user --collect --unit=omnia-t75-owner \
#         -E OWNER_RUN_PASSFILE=/run/user/$UID/t75-pw \
#         bash ~/omnia/docs/measurements/T7.5/owner-run.sh --remote

main() {
    set -euo pipefail
    trap '' HUP
    local root check=0 remote=0 arg ref="${OWNER_RUN_REF:-origin/main}"
    root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
    for arg in "$@"; do
        case $arg in
            --check) check=1 ;;
            --remote) remote=1 ;;
            *) echo "usage: owner-run.sh [--check] [--remote]" >&2; return 2 ;;
        esac
    done
    if ((remote)) && [[ -n ${OWNER_RUN_PASSFILE:-} ]]; then
        trap 'rm -f -- "$OWNER_RUN_PASSFILE"' EXIT
    fi

    local blockers=() supply mains=0 busy tool
    if ((remote == 0)) && [[ -n ${SSH_CONNECTION:-}${SSH_TTY:-} ]]; then
        blockers+=("started over SSH: run it at Orion's desk, or see --remote")
    fi
    if ((remote && !check)) && [[ ! -r ${OWNER_RUN_PASSFILE:-} ]]; then
        blockers+=("--remote needs OWNER_RUN_PASSFILE naming a readable sudo password file (--remote --check does not)")
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

    local out final
    final="$HOME/omnia-t75-owner-$(date -u +%Y%m%dT%H%M%SZ)"
    out=$(mktemp -d /tmp/omnia-t75-live.XXXXXX)
    mkdir "$final"
    exec 3>&1 4>&2
    exec > >(exec tee -i --output-error=warn -a "$out/owner-run.log") 2>&1
    local tee_pid=$!
    echo "owner-run: live output $out; copied to $final after the gate"

    local branch="${ref#origin/}"
    git -C "$root" fetch --quiet origin "$branch"
    git -C "$root" checkout --quiet --detach "$ref"
    local commit
    commit=$(git -C "$root" rev-parse HEAD)

    local governors tailscale_was mode=desk
    governors=$(grep -H . /sys/devices/system/cpu/cpu[0-9]*/cpufreq/scaling_governor)
    tailscale_was=$(systemctl is-active tailscaled 2>/dev/null || true)
    if ((remote)); then
        mode="remote: tailscaled and the starting SSH link kept running"
        tailscale_was="kept running ($tailscale_was)"
    fi
    {
        echo "mode: $mode"
        echo "ref: $ref"
        echo "commit $commit"
        echo "governors before: $(echo "$governors" | tr '\n' ' ')"
        echo "tailscaled before: $tailscale_was"
        echo "kernel $(uname -r); started $(date -u +%FT%TZ)"
        echo "baloo before:"
        balooctl6 status 2>&1 || true
    } > "$out/conditions.txt"

    echo "owner-run: sudo is needed once, now, for the governor and tailscaled"
    if ((remote)); then
        local sudo_ok=0
        sudo -S -p '' -v < "$OWNER_RUN_PASSFILE" && sudo_ok=1
        rm -f "$OWNER_RUN_PASSFILE"
        if ((sudo_ok == 0)); then echo "owner-run: sudo refused the password" >&2; return 2; fi
    else
        sudo -v
    fi
    : > "$final/helper.log"
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
        ok=1
        for path in /sys/devices/system/cpu/cpu[0-9]*/cpufreq/scaling_governor; do
            [[ $(cat "$path" 2>/dev/null) == performance ]] || ok=0
        done
        if [[ $TAILSCALE_WAS == active && $(systemctl is-active tailscaled) == active ]]; then
            ok=0
        fi
        if ((ok == 0)); then
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
    ' >> "$final/helper.log" 2>&1 &
    local helper=$! waited=0
    until grep -q -x -e ready -e failed "$final/helper.log"; do
        if ! kill -0 "$helper" 2>/dev/null || ((waited >= 30)); then break; fi
        sleep 1
        waited=$((waited + 1))
    done
    if ! grep -q -x ready "$final/helper.log"; then
        echo "owner-run: the root helper did not get ready; see $final/helper.log" >&2
        return 2
    fi

    echo "owner-run: building $ref $commit"
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
    if ! all_performance; then
        echo "owner-run: the performance governor is not set on every CPU" >&2
        return 2
    fi
    busy=$(busy_processes)
    if [[ -n $busy ]]; then
        printf 'owner-run: started during the build; close them and run again:\n%s\n' "$busy" >&2
        return 2
    fi

    cpu_times > "$out/cpu-before.json"
    local wake_out wake_rc=0
    wake_out=$(qdbus6 org.freedesktop.ScreenSaver /ScreenSaver SimulateUserActivity 2>&1) || wake_rc=$?
    {
        echo "SimulateUserActivity before inhibit: exit $wake_rc${wake_out:+ ($wake_out)}"
    } >> "$out/conditions.txt"
    sleep 1
    kde-inhibit --power --screenSaver --colorCorrect --notifications bash -c '
        sleep 6
        {
            echo "PowerManagement.HasInhibit: $(qdbus6 org.freedesktop.PowerManagement /org/freedesktop/PowerManagement/Inhibit HasInhibit 2>&1 || true)"
            echo "PolicyAgent.HasInhibition(1): $(qdbus6 org.kde.Solid.PowerManagement.PolicyAgent /org/kde/Solid/PowerManagement/PolicyAgent HasInhibition 1 2>&1 || true)"
            echo "PolicyAgent.HasInhibition(4): $(qdbus6 org.kde.Solid.PowerManagement.PolicyAgent /org/kde/Solid/PowerManagement/PolicyAgent HasInhibition 4 2>&1 || true)"
            echo "Brightness/dimming:"
            kscreen-doctor -o 2>&1 | grep -iE "brightness|dim" || true
        } > "$1/inhibit.txt"
        python3 -B "$2" run "$1/gate"
        echo $? > "$1/gate-exit.txt"
    ' owner-run "$out" "$root/docs/measurements/T7.5/gate.py" || true
    cpu_times > "$out/cpu-after.json"
    local status
    status=$(cat "$out/gate-exit.txt" 2>/dev/null || echo 2)
    {
        echo "finished $(date -u +%FT%TZ), gate exit $status"
        echo "inhibition 6 s into the inhibitor:"
        cat "$out/inhibit.txt" 2>/dev/null || true
        echo "brightness/dimming after:"
        kscreen-doctor -o 2>&1 | grep -iE "brightness|dim" || true
        echo "baloo after:"
        balooctl6 status 2>&1 || true
    } >> "$out/conditions.txt"

    echo
    echo "owner-run: gate exit $status ($(cat "$out/gate/status.txt" 2>/dev/null || echo 'no status'))"
    exec 1>&3 2>&4 3>&- 4>&-
    wait "$tee_pid" || true
    cp -a "$out/." "$final/"
    echo "owner-run: evidence in $final (live copy in $out)"
    echo "owner-run: the helper restores the governor and tailscaled on exit"
    return "$status"
}

all_performance() {
    local path
    for path in /sys/devices/system/cpu/cpu[0-9]*/cpufreq/scaling_governor; do
        [[ $(cat "$path" 2>/dev/null) == performance ]] || return 1
    done
}

baloo_idle() {
    local status
    status=$(LC_ALL=C balooctl6 status 2>&1) || return 1
    if grep -q -E 'is (currently )?disabled|is not running' <<< "$status"; then return 0; fi
    grep -q -x 'Baloo File Indexer is running' <<< "$status" \
        && grep -q -x 'Indexer state: Idle' <<< "$status" \
        && grep -q -x 'Files waiting for content indexing: 0' <<< "$status"
}

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
import json, os, time
procs = []
for pid in sorted((p for p in os.listdir("/proc") if p.isdigit()), key=int):
    try:
        with open(f"/proc/{pid}/stat") as f:
            rest = f.read().rsplit(")", 1)[1].split()
        utime, stime = int(rest[11]), int(rest[12])
        with open(f"/proc/{pid}/comm") as f:
            comm = f.read().strip()
        procs.append({"pid": int(pid), "comm": comm, "ticks": utime + stime})
    except (OSError, IndexError, ValueError):
        continue
print(json.dumps({"utc": time.strftime("%FT%TZ", time.gmtime()), "procs": procs}))
'
}

# Kept on one line: bash reads a whole line before running it, so the checkout inside `main`
# cannot change what runs next if it replaces this file on disk.
main "$@"; exit $?
