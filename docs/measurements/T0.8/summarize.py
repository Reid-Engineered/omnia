"""Reproduce tables from raw JSON. No sample removal, interpolation, or threshold changes.

Threshold rows: T-quad and T-present use absolute medians of run p99s; T-sprite and T-text
subtract the median of the interleaved baseline p99s (added cost). T-path divides the S4
paint p99 median by the path count; T-replay is the cached/repaint draw p99 ratio.
"""
import collections
import json
from pathlib import Path
from statistics import median

root = Path(__file__).parent
groups = collections.defaultdict(list)
s8_reports = []
for path in root.rglob('*.json'):
    if not path.name.startswith(('baseline-', 's1-', 's3-', 's4-', 's5-', 's6-', 's8-')):
        continue
    report = json.loads(path.read_text())
    if report['kind'] in ['run', 'spike']:
        # A degraded run is not good data.
        assert report['warnings'] == [], (path, report['warnings'])
        assert report['frames']['drawn'] == report['config']['frames'], (path, report['frames'])
    if path.name.startswith('s8-0-fixed-'):
        s8_reports.append(report)
    if 'draw_ms' in report.get('metrics', {}):
        key = str(path.relative_to(root).with_suffix('')).rsplit('-', 1)[0]
        groups[key].append(report)


def values(key, metric='draw_ms'):
    return [r['metrics'][metric]['p99'] for r in groups[key]]


def warm_paint_allocs(report, index):
    config = report['config']
    start, end = config['warmup'], config['warmup'] + config['frames']
    rows = [row for row in report['scene_details']['paint_trace'] if start <= row[0] < end]
    if not rows:
        raise ValueError('Missing warm paint trace')
    return max(row[index] for row in rows)


def med(key, metric='draw_ms'):
    return median(values(key, metric))


def spread(numbers):
    return f'{median(numbers):.3f} [{min(numbers):.3f}, {max(numbers):.3f}]'


for scene in ['baseline', 's1', 's3', 's4', 's5', 's6', 's8', 's125']:
    rows = []
    for key, reports in sorted(groups.items()):
        leaf = key.split('/')[-1]
        if scene == 'baseline':
            match = leaf.startswith('baseline')
        elif scene == 's125':
            match = key.startswith('s125/') and not leaf.startswith('baseline')
        elif scene == 's8':
            match = key.startswith('s8/')
        else:
            match = leaf.startswith(scene + '-') and not key.startswith('s125/')
        if match:
            rows.append((key, reports))
    if not rows:
        continue
    print('\n### ' + scene.upper() + '\n')
    print('| Case | Runs | Window::draw p99 median [min, max], ms |')
    print('| --- | ---: | ---: |')
    for key, reports in rows:
        print(f'| {key} | {len(reports)} | {spread(values(key))} |')

if s8_reports:
    print('\n### S8 startup and idle\n')
    print('| Metric | Probes | Median [min, max] |')
    print('| --- | ---: | ---: |')
    for label, samples in [
        ('Startup per-probe median, ms', [r['start']['metrics']['start_ms']['median'] for r in s8_reports]),
        ('Idle CPU, %', [r['idle']['metrics']['cpu_percent'] for r in s8_reports]),
        ('Idle private resident memory, MiB', [r['idle']['metrics']['memory_mb'] for r in s8_reports]),
        ('Idle working set, MiB', [r['idle']['metrics']['working_set_mb'] for r in s8_reports]),
    ]:
        print(f'| {label} | {len(samples)} | {spread(samples)} |')
    frames = sorted({r['idle']['metrics']['frames'] for r in s8_reports})
    print(f'\nIdle frame counts: {frames}. Each startup probe contains ten fresh process starts; '
          'this table summarizes the ten per-probe medians, not startup p99s or an OS-cache-cold start. '
          'CPU tick resolution is 10 ms on this host, about 0.1% over ten seconds. '
          'Readback is explicitly unsupported in every Linux probe.\n')


needed = ['s1-25000-quads-only', 's1-70000-quads-only', 'rest/baseline',
          'rest/s3-15000-fixed', 'rest/s5-2000-fixed', 'rest/s5-2000-direct',
          'rest/s6-60000-cached', 'rest/s6-60000-repaint', 'rest/s1-18999-quads-only']
if not all(key in groups for key in needed):
    raise SystemExit('Threshold tables require the remaining scene results.')

print('\n## Threshold comparisons\n')
print('Added cost = median scene p99 minus median of its interleaved baseline p99s. '
      'No negative differences are clipped. Replay ratio uses median draw p99s. '
      'S5 2,000 labels contain 12,000 glyphs.\n')
print('| Threshold | Limit | Measured | Result |')
print('| --- | --- | --- | --- |')
threshold_rows = []


def row(label, actual, limit, units, detail=''):
    outcome = 'PASS' if actual <= limit else 'FAIL'
    print(f'| {label} | ≤ {limit:.3f} {units} | {actual:.3f} {units}{detail} | {outcome} |')
    threshold_rows.append(dict(label=label, measured=actual, limit=limit, units=units, outcome=outcome))


for prefix, scale in [('', '100%'), ('s125/', '125%')]:
    for count, limit in [(25000, 3), (70000, 9)]:
        key = f'{prefix}s1-{count}-quads-only'
        if key in groups:
            row(f'T-quad {count:,} ({scale})', med(key), limit, 'ms')
base = med('rest/baseline')
row('T-sprite throughput, 15k', med('rest/s3-15000-fixed') - base, 2, 'ms')
print('\nRotated/mirrored image quality and the second-sweep atlas cap were not measured '
      '(Linux window readback unsupported; sweeps excluded by the task). '
      'Throughput failure alone fails T-sprite.\n')
print('| Threshold | Limit | Measured | Result |')
print('| --- | --- | --- | --- |')
for mode in ['fixed', 'direct']:
    row(f'T-text, 12k glyphs ({mode})', med(f'rest/s5-2000-{mode}') - base, 2, 'ms')
path_costs = [(med(f'rest/s4-{count}-fixed', 'paint_ms')*1000/count, count)
              for count in [100, 500, 1500, 5000]]
worst, count = max(path_costs)
row(f'T-path, worst tested per-path median ({count} paths)', worst, 3, 'µs/path')
row('T-replay, 60k', med('rest/s6-60000-cached') / med('rest/s6-60000-repaint') * 100, 40, '%')
present = 'rest/s1-18999-quads-only'
refresh = median(r['environment']['refresh_hz'] for r in groups[present])
row('T-present, exactly 19k quads including paper', med(present, 'dirty_to_present_ms'), 1000/refresh, 'ms')
print('\nT-present run p99 spread: ' + spread(values(present, 'dirty_to_present_ms')) + ' ms. '
      'This is GPUI CPU submission timing, not measured scanout latency.\n')
print('| S4 paths | Paint p99 median / count, µs/path | Max own allocations/frame | Max total allocations/frame |')
print('| ---: | ---: | ---: | ---: |')
for cost, count in path_costs:
    reports = groups[f'rest/s4-{count}-fixed']
    own = max(r['metrics']['allocs_per_frame']['ours']['max'] for r in reports)
    total = max(r['metrics']['allocs_per_frame']['total']['max'] for r in reports)
    print(f'| {count} | {cost:.3f} | {own:g} | {total:g} |')
print('\n| S5 labels/mode | Max own allocations/frame | Max total allocations/frame |')
print('| --- | ---: | ---: |')
for count in [2000, 6000, 12000]:
    for mode in ['fixed', 'direct']:
        reports = groups[f'rest/s5-{count}-{mode}']
        own = max(r['metrics']['allocs_per_frame']['ours']['max'] for r in reports)
        total = max(r['metrics']['allocs_per_frame']['total']['max'] for r in reports)
        print(f'| {count}/{mode} | {own:g} | {total:g} |')
print('\n| Warm S5 paint only | Max own allocations/frame | Max total allocations/frame |')
print('| --- | ---: | ---: |')
for count in [2000, 6000, 12000]:
    for mode in ['fixed', 'direct']:
        reports = groups[f'rest/s5-{count}-{mode}']
        own = max(warm_paint_allocs(r, 3) for r in reports)
        total = max(warm_paint_allocs(r, 4) for r in reports)
        print(f'| {count}/{mode} | {own:g} | {total:g} |')
print('\nFrame-level GPUI overhead above is separate from the warm label paint trace.\n')
print('\nPath/text allocation scopes follow Q5: GPUI calls suspend own counting; total '
      'is retained explicitly. Timing failures do not depend on allocation interpretation.\n')
print('| S6 60k mode | Scene paint calls per run |')
print('| --- | --- |')
for mode in ['cached', 'repaint']:
    calls = sorted({r['scene_details']['paint_calls'] for r in groups[f'rest/s6-60000-{mode}']})
    print(f'| {mode} | {calls} |')
(root / 'thresholds.json').write_text(json.dumps(threshold_rows, indent=2)+'\n')
