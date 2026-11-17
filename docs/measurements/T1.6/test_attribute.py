"""Tests for the B3/B4 missed-refresh classifier (attribute.py; canvas-plan.md 2.1,
owner decision 2026-11-17). Standard library; run: python3 docs/measurements/T1.6/test_attribute.py
"""
import importlib.util
import json
import pathlib
import tarfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('attribute', ROOT / 'attribute.py')
attribute = importlib.util.module_from_spec(spec)
spec.loader.exec_module(attribute)

ARCHIVE = ROOT / 'traced/results-2026-11-17/attempt-LBKFmccq-data.tar.xz'
MS = 1_000_000
HZ = 50.0  # one refresh = exactly 20 ms, so the boundaries are exact integers
R = 20 * MS


def real_report(member):
    with tarfile.open(ARCHIVE, 'r:xz') as archive:
        return json.load(archive.extractfile(member))


def synthetic(late=None, frames=10, warmup=2):
    """A steady 20 ms run; `late=(index, gap_ns, work_ns)` delays one measured frame.

    `gap_ns` is previous present end to the late frame's signal; `work_ns` is that signal
    to its present end. The bench count is the true count of intervals over 1.5 refreshes.
    """
    events, end, missed = [], 0, 0
    for ordinal in range(warmup + frames):
        index = ordinal - warmup
        gap, work = (late[1], late[2]) if late and index == late[0] else (R - 2 * MS, 2 * MS)
        signal = end + gap
        draw_end = signal + work // 2
        events.append(dict(event='draw', window='W', signal_ns=signal, dirty_ns=signal,
                           start_ns=signal, end_ns=draw_end))
        present_end = signal + work
        events.append(dict(event='present', window='W', start_ns=draw_end, end_ns=present_end,
                           animation_interval_ns=None if ordinal == 0 else present_end - end))
        if index >= 0 and present_end - end > 1.5 * R:
            missed += 1
        end = present_end
    return {
        'kind': 'run', 'config': dict(warmup=warmup, frames=frames, diagnostic_trace=True),
        'environment': dict(refresh_hz=HZ),
        'metrics': dict(missed_refreshes=missed, present_intervals_over_2_5_refreshes=0),
        'diagnostic_trace': dict(units='nanoseconds since trace creation', events=events),
    }


class AttributeTests(unittest.TestCase):
    def test_real_zoom_6_miss_is_compositor_side(self):
        # Traced campaign 2026-11-17, 125% zoom run 6: frame 195, 33.13 ms interval.
        result = attribute.attribute(real_report('./s125/zoom-6.json'))
        self.assertTrue(result['usable'], result['error'])
        self.assertEqual(result['aggregate'], 1)
        [miss] = result['misses']
        self.assertEqual(miss['index'], 195)
        self.assertAlmostEqual(miss['interval_ms'], 33.130447, places=6)
        self.assertAlmostEqual(miss['gap_ms'], 31.968034, places=6)
        self.assertAlmostEqual(miss['work_ms'], 1.162413, places=6)
        self.assertEqual(miss['verdict'], attribute.COMPOSITOR)
        self.assertEqual((result['compositor'], result['counted'], result['passed']), (1, 0, True))

    def test_real_clean_run_has_no_misses(self):
        result = attribute.attribute(real_report('./s125/pan-1.json'))
        self.assertEqual((result['usable'], result['misses'], result['counted']), (True, [], 0))

    def test_slow_draw_is_counted(self):
        # Signal on time, then 30 ms of Omnia work: an Omnia-side miss.
        report = synthetic(late=(5, R - 2 * MS, 30 * MS))
        result = attribute.attribute(report)
        self.assertTrue(result['usable'], result['error'])
        [miss] = result['misses']
        self.assertEqual(miss['verdict'], attribute.COUNTED)
        self.assertIn('(b)', miss['reason'])
        self.assertEqual((result['counted'], result['passed']), (1, False))

    def test_late_signal_and_slow_draw_is_counted(self):
        # Compositor late and Omnia slow: (c) fails, so it counts.
        result = attribute.attribute(synthetic(late=(5, 25 * MS, 21 * MS)))
        [miss] = result['misses']
        self.assertEqual(miss['verdict'], attribute.COUNTED)
        self.assertIn('(c)', miss['reason'])

    def test_missing_trace_fails_closed(self):
        report = synthetic(late=(5, 30 * MS, 2 * MS))
        del report['diagnostic_trace']
        result = attribute.attribute(report)
        self.assertFalse(result['usable'])
        self.assertFalse(result['passed'])
        self.assertIn('missing', result['error'])
        # Even a run whose bench count is zero fails without a trace.
        clean = synthetic()
        clean['diagnostic_trace'] = None
        self.assertFalse(attribute.attribute(clean)['passed'])

    def test_truncated_trace_fails_closed(self):
        report = real_report('./s125/zoom-6.json')
        report['diagnostic_trace']['events'] = report['diagnostic_trace']['events'][:-40]
        result = attribute.attribute(report)
        self.assertFalse(result['usable'])
        self.assertFalse(result['passed'])

    def test_missing_signal_counts(self):
        report = synthetic(late=(5, 30 * MS, 2 * MS))
        late = [e for e in report['diagnostic_trace']['events'] if e['event'] == 'draw'][2 + 5]
        late['signal_ns'] = None
        [miss] = attribute.attribute(report)['misses']
        self.assertEqual(miss['verdict'], attribute.COUNTED)

    def test_untraced_bench_count_counts(self):
        # The bench counted an interval the trace does not show: it lacks a trace, so it counts.
        report = synthetic()
        report['metrics']['missed_refreshes'] = 1
        result = attribute.attribute(report)
        self.assertTrue(result['usable'])
        self.assertEqual((result['counted'], result['passed']), (1, False))

    def test_bench_count_below_trace_fails_closed(self):
        report = synthetic(late=(5, 30 * MS, 2 * MS))
        report['metrics']['missed_refreshes'] = 0
        result = attribute.attribute(report)
        self.assertFalse(result['usable'])
        self.assertFalse(result['passed'])

    def test_boundary_b_gap_of_exactly_one_refresh_counts(self):
        # (b) needs the gap strictly over one refresh. Interval 20 + 19 = 39 ms > 30 ms.
        [miss] = attribute.attribute(synthetic(late=(5, R, R - MS)))['misses']
        self.assertEqual(miss['verdict'], attribute.COUNTED)
        self.assertIn('(b)', miss['reason'])
        [miss] = attribute.attribute(synthetic(late=(5, R + 1, R - MS)))['misses']
        self.assertEqual(miss['verdict'], attribute.COMPOSITOR)

    def test_boundary_c_work_of_exactly_one_refresh_counts(self):
        # (c) needs signal-to-present-end strictly under one refresh.
        [miss] = attribute.attribute(synthetic(late=(5, 25 * MS, R)))['misses']
        self.assertEqual(miss['verdict'], attribute.COUNTED)
        self.assertIn('(c)', miss['reason'])
        [miss] = attribute.attribute(synthetic(late=(5, 25 * MS, R - 1)))['misses']
        self.assertEqual(miss['verdict'], attribute.COMPOSITOR)

    def test_interval_of_exactly_1_5_refreshes_is_not_a_miss(self):
        result = attribute.attribute(synthetic(late=(5, 28 * MS, 2 * MS)))
        self.assertEqual(result['misses'], [])
        result = attribute.attribute(synthetic(late=(5, 28 * MS, 2 * MS + 1)))
        self.assertEqual(len(result['misses']), 1)

    def test_unpresented_draw_in_measured_frames_fails_closed(self):
        # A measured draw that is never presented leaves the trace without a present for
        # every measured frame: the run cannot be attributed and fails closed.
        report = synthetic(late=(5, 30 * MS, 2 * MS))
        events = report['diagnostic_trace']['events']
        late_at = [i for i, e in enumerate(events) if e['event'] == 'draw'][2 + 5]
        prev_end = events[late_at - 1]['end_ns']
        events.insert(late_at, dict(event='draw', window='W', signal_ns=prev_end + MS,
                                    dirty_ns=None, start_ns=prev_end + MS,
                                    end_ns=prev_end + 2 * MS))
        report['config']['frames'] += 1  # the bench drew one more frame
        result = attribute.attribute(report)
        self.assertEqual((result['usable'], result['passed']), (False, False))

    def test_two_draws_since_previous_present_count(self):
        # Direct check of the one-draw condition (Omnia was not idle during the wait).
        draw = dict(signal_ns=30 * MS, start_ns=30 * MS, end_ns=31 * MS)
        present = dict(start_ns=31 * MS, end_ns=32 * MS)
        prev = dict(end_ns=0)
        self.assertEqual(attribute.classify((5, draw, present, prev, 1), R)[0], attribute.COMPOSITOR)
        verdict, reason = attribute.classify((5, draw, present, prev, 2), R)
        self.assertEqual(verdict, attribute.COUNTED)
        self.assertIn('2 draws', reason)

    def test_out_of_order_timestamps_count(self):
        draw = dict(signal_ns=30 * MS, start_ns=29 * MS, end_ns=31 * MS)
        present = dict(start_ns=31 * MS, end_ns=32 * MS)
        verdict, _ = attribute.classify((5, draw, present, dict(end_ns=0), 1), R)
        self.assertEqual(verdict, attribute.COUNTED)

    def test_b4_limit_uses_the_same_criterion(self):
        # B4: interval over 2.5 refreshes (50 ms here); the same (a), (b), (c) decide.
        report = synthetic(late=(5, 50 * MS, 2 * MS))
        report['metrics']['present_intervals_over_2_5_refreshes'] = 1
        result = attribute.attribute(report, 2.5)
        [miss] = result['misses']
        self.assertEqual((miss['verdict'], result['counted']), (attribute.COMPOSITOR, 0))
        report = synthetic(late=(5, R - 2 * MS, 40 * MS))
        report['metrics']['present_intervals_over_2_5_refreshes'] = 1
        [miss] = attribute.attribute(report, 2.5)['misses']
        self.assertEqual(miss['verdict'], attribute.COUNTED)


if __name__ == '__main__':
    unittest.main()
