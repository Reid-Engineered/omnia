"""T1.6 / audit 4.1 diagnostic analysis. Standard library; never a gate verdict.

Usage: python analyze.py ATTEMPT_OR_JSON [MORE_PATHS...]
Missing traces do not establish absence of misses. Raw trace intervals, rather
than histogram bucket tops, determine reproduced misses.
"""
import gzip
import json
import pathlib
import re
import sys


def timeline(report):
    """Mirror pending-frame replacement; count draws, not present events."""
    events = report['diagnostic_trace']['events']
    warmup, frames = report['config']['warmup'], report['config']['frames']
    pending, ordinal, previous = {}, -1, {}
    rows = []
    for event in events:
        window = event['window']
        if event['event'] == 'draw':
            ordinal += 1
            pending[window] = (ordinal, event)
        elif event['event'] == 'present':
            if window not in pending:
                raise ValueError('present without a pending draw')
            index, draw = pending.pop(window)
            rows.append((index - warmup, draw, event, previous.get(window)))
            previous[window] = event
    measured = [row for row in rows if 0 <= row[0] < frames]
    if ordinal + 1 != warmup + frames or len(measured) != frames:
        raise ValueError('trace truncated or unexpected draw/present count; index unavailable')
    for _, _, present, prev in measured:
        interval = present.get('animation_interval_ns')
        if interval is None or prev is None or present['end_ns'] - prev['end_ns'] != interval:
            raise ValueError('missing/inconsistent raw interval; analysis incomplete')
    return rows, measured


def wayland_lines(path):
    lines = []
    epoch, previous = {}, {}
    opener = gzip.open if path.suffix == '.gz' else open
    with opener(path, 'rt', errors='replace') as stream:
        raw = stream.read()
    for number, line in enumerate(raw.splitlines(), 1):
        tod = re.search(r'\[(\d{2}):(\d{2}):(\d{2}\.\d+)\]', line)
        match = re.search(r'\[\s*(\d+\.\d+)\]', line)
        if tod:
            clock = 'time-of-day'
            stamp = (int(tod[1]) * 3600 + int(tod[2]) * 60 + float(tod[3])) * 1000
            period, wrap = 86_400_000, 43_200_000
        elif match:
            clock = 'milliseconds'
            stamp = float(match[1])
            period, wrap = 2**32, 2**31
        else:
            continue
        # Time-of-day wraps at midnight; numeric client timestamps wrap at 2^32 ms.
        if previous.get(clock, stamp) - stamp > wrap:
            epoch[clock] = epoch.get(clock, 0) + period
        previous[clock] = stamp
        lines.append(((stamp + epoch.get(clock, 0)) * 1e6, number, line))
    return lines


def correlate(path, rows, low, high):
    if not path.exists():
        print('  Wayland log unavailable; callback correlation unavailable')
        return
    lines = wayland_lines(path)
    # No shared clock anchor is emitted by trace.rs. A surface commit must be
    # inside its CPU platform-present bracket. Require one unique contiguous
    # sequence whose offset bounds intersect across ALL traced presents. Never
    # assume a callback-to-frame bijection (retry timer and startup exist).
    surfaces = {}
    for stamp, number, line in lines:
        match = re.search(r'-> wl_surface[@#](\d+)\.commit\(', line)
        if match:
            surfaces.setdefault(match[1], []).append(stamp)
    candidates = []
    presents = [row[2] for row in rows]
    for surface, commits in surfaces.items():
        for start in range(len(commits) - len(presents) + 1):
            lower, upper = -float('inf'), float('inf')
            for stamp, present in zip(commits[start:], presents):
                # Debug timestamps have microsecond precision: +/- 1 us.
                lower = max(lower, stamp - present['end_ns'] - 1000)
                upper = min(upper, stamp - present['start_ns'] + 1000)
                if lower > upper:
                    break
            else:
                candidates.append((surface, lower, upper))
    traffic = [(t, n, line) for t, n, line in lines if re.search(
        r'wl_surface[@#]\d+\.frame\(|wl_callback[@#]\d+\.done\(|'
        r'wp_presentation|wp_presentation_feedback', line)]
    if len(candidates) != 1:
        print(f'  Wayland correlation unresolved ({len(candidates)} clock alignments); '
              'callback traffic follows in log order on its OWN clock, not assigned to this miss:')
        for _, number, line in traffic:
            print(f'    {path.name}:{number}: {line}')
        return
    surface, lower, upper = candidates[0]
    print(f'  Candidate Wayland surface {surface}; log-minus-trace clock offset '
          f'[{lower / 1e6:.6f}, {upper / 1e6:.6f}] ms; temporal association only')
    selected = [(n, line) for t, n, line in traffic
                if low + lower - 2_000_000 <= t <= high + upper + 2_000_000]
    for number, line in selected:
        print(f'    {path.name}:{number}: {line}')
    if not selected:
        print('    no callback/presentation feedback traffic in correlated window')
    return surface, lower, upper


def analyze(path):
    report = json.loads(path.read_text())
    if report.get('kind') != 'run' or report.get('config', {}).get('script') not in ('pan', 'zoom'):
        return 0, 0, 0, 0
    print(f'{path}: diagnostic motion run')
    if not report.get('diagnostic_trace'):
        metrics = report.get('metrics', {})
        print('  diagnostic_trace missing; measured index and callback correlation unavailable; '
              f"aggregate missed_refreshes={metrics.get('missed_refreshes', 'unknown')}")
        return 1, 0, 0, 0
    rows, measured = timeline(report)
    threshold = 1.5e9 / report['environment']['refresh_hz']
    misses = [row for row in measured if row[2]['animation_interval_ns'] > threshold]
    print(f'  raw misses={len(misses)}; threshold={threshold / 1e6:.6f} ms; '
          f"histogram count={report.get('metrics', {}).get('missed_refreshes', 'unknown')} (bucket upper bound)")
    disagreement = not misses and report.get('metrics', {}).get('missed_refreshes', 0) > 0
    if disagreement:
        print('  aggregate/raw disagreement: aggregate missed_refreshes > 0 but no raw miss; '
              'possible bucket-top overcount (audit 2.3.5); run is not counted as clean')
    for index, draw, present, previous in misses:
        print(f"  measured_index={index}; interval_ms={present['animation_interval_ns'] / 1e6:.6f}; "
              f"platform_present_ms={(present['end_ns'] - present['start_ns']) / 1e6:.6f}")
        if index == 0:
            print('  index-0 warmup/baseline boundary: observation only; no baseline cause established')
        nearby = [row for row in rows if index - 1 <= row[0] <= index + 1]
        stamps = [(previous['end_ns'], 'previous present end')]
        for number, d, p, _ in nearby:
            for key, label in [('signal_ns', 'signal'), ('dirty_ns', 'dirty'),
                               ('start_ns', 'draw start'), ('end_ns', 'draw end')]:
                if d.get(key) is not None:
                    stamps.append((d[key], f'index {number} {label}'))
                else:
                    print(f'    index {number} {label}: unavailable')
            stamps.extend([(p['start_ns'], f'index {number} present start'),
                           (p['end_ns'], f'index {number} present end')])
        for ns, label in sorted(set(stamps)):
            print(f'    {ns:>16} ns  {label}')
        correlate(path.with_suffix('.wayland.log'), rows, previous['end_ns'], present['end_ns'])
    return 1, 1, len(misses), int(disagreement)


def main(args):
    paths = set()
    for arg in args:
        path = pathlib.Path(arg)
        paths.update(path.rglob('*.json') if path.is_dir() else [path])
    counts = [0, 0, 0, 0]
    errors = 0
    print('DIAGNOSTIC ONLY - not a gate verdict; no causal attribution')
    for path in sorted(paths):
        try:
            result = analyze(path)
            counts = [a + b for a, b in zip(counts, result)]
        except (ValueError, KeyError, TypeError, OSError, ZeroDivisionError) as error:
            errors += 1
            print(f'{path}: analysis incomplete: {error}')
    runs, traced, misses, disagreements = counts
    root = pathlib.Path(args[0]) if len(args) == 1 else None
    status = root / 'status.txt' if root is not None and root.is_dir() else None
    complete = status is not None and status.exists() and status.read_text().strip() == 'complete'
    expected = {root / tag / f'{case}-{rep}.json'
                for tag in ('s100', 's125') for case in ('pan', 'zoom') for rep in range(1, 11)} if root else set()
    complete = complete and expected.issubset(paths) and runs == traced == 40 and not errors
    if not complete:
        print('incomplete diagnostic dataset: require status.txt=complete and exactly 40 usable scheduled motion traces')
    if complete and misses == 0 and disagreements == 0:
        print(f'not reproduced in {runs} motion runs')
    else:
        print(f'{runs} motion runs; {traced} usable traces; {misses} raw misses; '
              f'{runs - traced} missing traces; {errors} analysis errors; {disagreements} aggregate/raw disagreements; '
              'absence of reproduction cannot be asserted for missing/incomplete traces')
    return int(bool(errors))


if __name__ == '__main__':
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    sys.exit(main(sys.argv[1:]))
