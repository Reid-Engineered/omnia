"""Attribute B3/B4 missed refreshes in a traced `bench run` report (canvas-plan 2.1).

Owner decision 2026-11-17: a refresh that the compositor skips while Omnia is idle does not
count as a missed refresh against B3, and the same exclusion applies to B4's interval rule.
This module applies the mechanical criterion recorded in canvas-plan.md section 2.1. A
present interval longer than `limit` refreshes is compositor-side, and excluded, only when
the run's trace shows all of:

  (a) the previous present had ended before the late frame's signal;
  (b) the late frame's signal arrived more than one refresh after that present end;
  (c) the late frame's signal-to-present-end time is under one refresh;
  (d) Omnia's main thread was idle for more than one refresh before the signal: the latest
      end (clipped to the signal) of any main-thread work in the GPUI foreground journal
      (`foreground`) that started before the signal, or the previous present end if later,
      is more than one refresh before the signal;
  (e) the signal is a native platform frame callback (`signal_source == "native"`), not a
      timer, retry or other locally scheduled request;

and, so that these describe one idle wait and one frame of work, the late present has
exactly one draw since the previous present, that draw has a signal, and its timestamps are
ordered: signal <= draw start <= draw end <= present start <= present end. Every other
interval over the limit counts. A run whose trace is missing or unusable fails closed
(including a trace without the journal or with journal entries lost), and so does a run
whose bench count of intervals over the limit is below the traced count.
Bench-counted intervals the trace does not show (`aggregate - traced`) count: they lack a
usable trace. Standard library only.
"""
import json
import pathlib
import sys

COMPOSITOR = 'compositor-side'
COUNTED = 'counted'
# The bench's own count of present intervals over each limit (crates/omnia-bench/src/harness.rs).
AGGREGATE = {1.5: 'missed_refreshes', 2.5: 'present_intervals_over_2_5_refreshes'}


class Unusable(ValueError):
    """The trace cannot attribute this run's intervals; the run fails closed."""


def timeline(report):
    """Pair each present with its draw, as traced/analyze.py does, and count every draw.

    Returns one row per measured frame: (index, draw, present, previous present, draws since
    the previous present). Draws that a later draw replaced before any present still count
    towards the next present's `draws`.
    """
    trace = report.get('diagnostic_trace')
    if not trace or not isinstance(trace.get('events'), list):
        raise Unusable('diagnostic_trace missing')
    if trace.get('units') != 'nanoseconds since trace creation':
        raise Unusable(f"trace units {trace.get('units')!r}")
    warmup, frames = report['config']['warmup'], report['config']['frames']
    pending, since, previous = {}, {}, {}
    ordinal, rows = -1, []
    for event in trace['events']:
        window = event['window']
        if event['event'] == 'draw':
            ordinal += 1
            pending[window] = (ordinal, event)
            since[window] = since.get(window, 0) + 1
        elif event['event'] == 'present':
            if window not in pending:
                raise Unusable('present without a pending draw')
            index, draw = pending.pop(window)
            rows.append((index - warmup, draw, event, previous.get(window), since.pop(window)))
            previous[window] = event
        else:
            raise Unusable(f"unknown trace event {event['event']!r}")
    measured = [row for row in rows if 0 <= row[0] < frames]
    if ordinal + 1 != warmup + frames or len(measured) != frames:
        raise Unusable('trace truncated or unexpected draw/present count')
    if not isinstance(trace.get('foreground'), list) or trace.get('journal_lost') != 0:
        raise Unusable('foreground journal missing or incomplete '
                       f"(journal_lost {trace.get('journal_lost')!r})")
    if len({row[1]['window'] for row in measured}) != 1:
        raise Unusable('measured frames span more than one window')
    for _, _, present, prev, _ in measured:
        interval = present.get('animation_interval_ns')
        if interval is None or prev is None or present['end_ns'] - prev['end_ns'] != interval:
            raise Unusable('missing or inconsistent present interval')
    return measured


def busy_until(foreground, after, signal):
    """Latest end, clipped to `signal`, of journal work that started before `signal`.

    Never earlier than `after` (the previous present end)."""
    ends = [min(work['end_ns'], signal) for work in foreground if work['start_ns'] < signal]
    return max([after] + ends)


def classify(row, refresh_ns, foreground):
    """Classify one interval already known to be over its limit. Returns (verdict, reason)."""
    _, draw, present, prev, draws = row
    signal = draw.get('signal_ns')
    if signal is None:
        return COUNTED, 'late frame has no signal time'
    if draws != 1:
        return COUNTED, f'{draws} draws since the previous present'
    if not signal <= draw['start_ns'] <= draw['end_ns'] <= present['start_ns'] <= present['end_ns']:
        return COUNTED, 'late frame timestamps out of order'
    gap = signal - prev['end_ns']
    work = present['end_ns'] - signal
    if not gap > 0:
        return COUNTED, '(a) fails: signal did not follow the previous present end'
    if not gap > refresh_ns:
        return COUNTED, '(b) fails: signal within one refresh of the previous present end'
    if not work < refresh_ns:
        return COUNTED, '(c) fails: signal to present end not under one refresh'
    idle = signal - busy_until(foreground, prev['end_ns'], signal)
    if not idle > refresh_ns:
        return COUNTED, f'(d) fails: main thread idle only {idle / 1e6:.3f} ms before the signal'
    source = draw.get('signal_source')
    if source != 'native':
        return COUNTED, f'(e) fails: signal source {source!r}, not a native frame callback'
    return COMPOSITOR, '(a) to (e) hold'


def attribute(report, limit=1.5):
    """Attribute every present interval over `limit` refreshes in one motion report.

    Returns a dict: usable, error, refresh_ns, aggregate, misses (list of per-interval dicts),
    compositor, counted, passed. `counted` includes bench-counted intervals the trace lacks.
    """
    result = dict(usable=False, error=None, refresh_ns=None, aggregate=None, misses=[],
                  compositor=0, counted=None, passed=False)
    try:
        hz = report['environment']['refresh_hz']
        if not hz or hz <= 0:
            raise Unusable(f'refresh_hz {hz!r}')
        refresh_ns = 1e9 / hz
        result['refresh_ns'] = refresh_ns
        aggregate = report.get('metrics', {}).get(AGGREGATE[limit])
        result['aggregate'] = aggregate
        if not isinstance(aggregate, int):
            raise Unusable(f'bench count {AGGREGATE[limit]} = {aggregate!r}')
        measured = timeline(report)
    except Unusable as error:
        result['error'] = str(error)
        return result
    except (KeyError, TypeError) as error:
        result['error'] = f'malformed report or trace: {error!r}'
        return result
    for row in measured:
        index, draw, present, prev, draws = row
        interval = present['animation_interval_ns']
        if not interval > limit * refresh_ns:
            continue
        foreground = report['diagnostic_trace']['foreground']
        verdict, reason = classify(row, refresh_ns, foreground)
        signal = draw.get('signal_ns')
        busy = None if signal is None else busy_until(foreground, prev['end_ns'], signal)
        result['misses'].append(dict(
            index=index, interval_ms=interval / 1e6, verdict=verdict, reason=reason,
            gap_ms=None if signal is None else (signal - prev['end_ns']) / 1e6,
            work_ms=None if signal is None else (present['end_ns'] - signal) / 1e6,
            idle_ms=None if signal is None else (signal - busy) / 1e6,
            source=draw.get('signal_source'),
            timeline=[(prev['end_ns'], 'previous present end'),
                      (signal, 'signal'), (draw.get('dirty_ns'), 'dirty'),
                      (draw['start_ns'], 'draw start'), (draw['end_ns'], 'draw end'),
                      (present['start_ns'], 'present start'), (present['end_ns'], 'present end')]))
    traced = len(result['misses'])
    if aggregate < traced:
        # The bench histogram can only over-count (stats::count_over_refreshes).
        result['error'] = f'bench count {aggregate} below traced count {traced}'
        return result
    result['usable'] = True
    result['compositor'] = sum(m['verdict'] == COMPOSITOR for m in result['misses'])
    result['counted'] = (traced - result['compositor']) + (aggregate - traced)
    result['passed'] = result['counted'] == 0
    return result


def describe(name, result):
    """Human-readable lines for one run's attribution."""
    if not result['usable']:
        return [f'{name}: FAIL (trace unusable: {result["error"]}; bench count '
                f'{result["aggregate"]}, every one counted)']
    untraced = result['aggregate'] - len(result['misses'])
    lines = [f'{name}: {"PASS" if result["passed"] else "FAIL"}; over limit '
             f'{len(result["misses"])} traced, {untraced} untraced; '
             f'compositor-side {result["compositor"]}, counted {result["counted"]}']
    for m in result['misses']:
        gap = 'n/a' if m['gap_ms'] is None else f'{m["gap_ms"]:.3f}'
        work = 'n/a' if m['work_ms'] is None else f'{m["work_ms"]:.3f}'
        idle = 'n/a' if m['idle_ms'] is None else f'{m["idle_ms"]:.3f}'
        lines.append(f'  frame {m["index"]}: interval {m["interval_ms"]:.3f} ms; present end to '
                     f'signal {gap} ms; main thread idle before signal {idle} ms; signal to '
                     f'present end {work} ms; source {m["source"]}; {m["verdict"]}: {m["reason"]}')
        base = m['timeline'][0][0]
        for ns, label in m['timeline']:
            stamp = 'n/a' if ns is None else f'{(ns - base) / 1e6:+10.3f} ms'
            lines.append(f'    {stamp}  {label}')
    if untraced:
        lines.append(f'  {untraced} bench-counted interval(s) without a traced interval: counted')
    return lines


if __name__ == '__main__':
    if len(sys.argv) < 2:
        sys.exit('usage: attribute.py REPORT.json [...]')
    ok = True
    for arg in sys.argv[1:]:
        r = attribute(json.loads(pathlib.Path(arg).read_text()))
        ok &= r['passed']
        print('\n'.join(describe(arg, r)))
    sys.exit(0 if ok else 1)
