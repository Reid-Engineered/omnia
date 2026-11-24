"""Tests for build_wiki.py. Run: python3 -m unittest discover -s .github/wiki"""

import re
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import build_wiki as wiki  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[2]

ROADMAP = """\
# Example: roadmap for agents

## Human checkpoints

| ID | When | What the owner does |
| --- | --- | --- |
| HX1 | X0.2 | Answers XQ1. |
| HX2 | During X1.2 | Approves goldens. |
| `HX3` | X1.7 | Written as code. |
| HX4 | Either \\| both | Has an escaped pipe. |
| — | Any time | Answers questions. |

## Milestone X0: decisions

### X0.1 Plan · done
- Lane: any · Depends: none · Read: everything

### X0.2 Owner decisions · done → HX1 (signed 2026-11-20)
- Lane: any · Depends: X0.1

## Milestone X1: work

### X1.1 First thing · todo
- Lane: doc · Depends: X0.2, X0.1 · Read: XD1
- Owns: `src/a.rs`

### X1.2 Second thing · needs-human → HX2
- Lane: doc · Depends: X1.1 (after the
  first thing lands) · Read: XD2

### X1.3 Third thing · todo (optional)
- Lane: doc · Depends: X1.2

### X1.4 Either way · todo
- Lane: doc · Depends: X0.1 or X0.2

### X1.5 Old thing · replaced by X1.4
- Lane: doc · Depends: X0.1

### X1.6 Outside · todo
- Lane: doc · Depends: Y9.9

### X1.7 Conditional · todo
- Lane: doc · Depends: X0.1 (and X1.4 if built)

### X1.8 No depends field · todo
- Lane: doc

## Log

Append one entry per finished task. Newest last.

```
<date> <task id> <status> — what was done
```

2026-11-20 X0.1 done — wrote the plan.

2026-11-21 X0.2 done — owner signed;
                       second line of the entry.
2026-11-21 X1.2 needs-human — prepared the gate for protocol
2025-11-25 sessions.
"""

OTHER = """\
# Other: roadmap for agents

## Milestone Y1: more

### Y1.1 Elsewhere · doing (half done)
- Lane: app · Depends: X0.2

## Log

2026-11-21 Y1.1 doing — started.
2026-11-22 Y1.1 doing — continued.
"""


def roadmap(text=ROADMAP, index=0):
    return wiki.parse_roadmap(index, "x-roadmap.md", "X-roadmap", "X", text)


class ParseRoadmap(unittest.TestCase):
    def test_statuses_checkpoints_and_milestones(self):
        r = roadmap()
        by_id = {t.id: t for t in r.tasks}
        self.assertEqual([t.id for t in r.tasks], ["X0.1", "X0.2", "X1.1", "X1.2", "X1.3", "X1.4", "X1.5", "X1.6", "X1.7", "X1.8"])
        self.assertEqual(by_id["X0.2"].status, "done")
        self.assertEqual(by_id["X0.2"].checkpoint, "HX1")
        self.assertEqual(by_id["X1.2"].status, "needs-human")
        self.assertEqual(by_id["X1.3"].status, "todo")
        self.assertEqual(by_id["X1.3"].status_text, "todo (optional)")
        self.assertEqual(by_id["X1.5"].status, "other")
        self.assertEqual(by_id["X1.1"].milestone, "Milestone X1: work")
        self.assertEqual(r.milestones, ["Milestone X0: decisions", "Milestone X1: work"])

    def test_bullet_fields_span_continuation_lines(self):
        by_id = {t.id: t for t in roadmap().tasks}
        self.assertEqual(by_id["X1.1"].lane, "doc")
        self.assertEqual(by_id["X1.1"].depends, "X0.2, X0.1")
        self.assertEqual(by_id["X1.2"].depends, "X1.1 (after the first thing lands)")

    def test_checkpoint_table_skips_unnamed_rows(self):
        checkpoints = roadmap().checkpoints
        self.assertEqual([c.id for c in checkpoints], ["HX1", "HX2", "HX3", "HX4"])
        self.assertEqual((checkpoints[3].when, checkpoints[3].what),
                         ("Either \\| both", "Has an escaped pipe."))

    def test_log_skips_the_template_and_joins_lines(self):
        log = roadmap().log
        self.assertEqual([(e.date, e.text.split(" ")[0]) for e in log],
                         [("2026-11-20", "X0.1"), ("2026-11-21", "X0.2"), ("2026-11-21", "X1.2")])
        self.assertEqual(log[1].text, "X0.2 done — owner signed; second line of the entry.")
        # A wrapped line that starts with a date is not a new entry.
        self.assertEqual(log[2].text, "X1.2 needs-human — prepared the gate for protocol 2025-11-25 sessions.")

    def test_extra_space_after_the_date_still_starts_an_entry(self):
        text = "## Log\n\n2026-11-23 S0.1 done — one.\n2026-11-23  S0.2 re-review — two.\n"
        self.assertEqual([e.text for e in roadmap(text).log], ["S0.1 done — one.", "S0.2 re-review — two."])

    def test_fenced_and_bullet_log_styles(self):
        text = (
            "## Log\n\n```\n2026-11-23 S0.1 done — wrote it;\n"
            "                       more.\n```\n\n"
            "- **2026-11-24 · P0.1/P0.2 · done (`HP0`):** Signed.\n"
            "- **2026-11-25 · P0.1 · review of `1d162f9`:** Fixed.\n"
        )
        self.assertEqual([(e.date, e.text) for e in roadmap(text).log], [
            ("2026-11-23", "S0.1 done — wrote it; more."),
            ("2026-11-24", "P0.1/P0.2 done (`HP0`) — Signed."),
            ("2026-11-25", "P0.1 review of `1d162f9` — Fixed."),
        ])

    def test_anchors_match_github_including_repeats(self):
        seen = {}
        self.assertEqual(
            wiki.github_anchor("W2.2 Move and Reorient · blocked (WB3: 51.2 ms)", seen),
            "w22-move-and-reorient--blocked-wb3-512-ms",
        )
        self.assertEqual(wiki.github_anchor("S4.4 Gate — `final` · todo → HS4", seen),
                         "s44-gate--final--todo--hs4")
        self.assertEqual(wiki.github_anchor("Log", seen), "log")
        self.assertEqual(wiki.github_anchor("Log", seen), "log-1")
        self.assertEqual(wiki.github_anchor("Log-1", seen), "log-1-1")
        self.assertEqual(wiki.github_anchor("Log", seen), "log-2")


class Readiness(unittest.TestCase):
    def setUp(self):
        self.tasks = {t.id: t for t in roadmap().tasks}

    def test_ready_when_every_named_task_is_done(self):
        self.assertEqual(wiki.readiness(self.tasks["X1.1"], self.tasks), "ready")

    def test_needs_human_dependency_is_not_done(self):
        self.assertEqual(wiki.readiness(self.tasks["X1.3"], self.tasks), "waiting")

    def test_wording_a_script_should_not_judge(self):
        self.assertEqual(wiki.readiness(self.tasks["X1.4"], self.tasks), "check")

    def test_unknown_task_needs_a_person(self):
        self.assertEqual(wiki.readiness(self.tasks["X1.6"], self.tasks), "check")

    def test_a_task_named_in_a_note_is_a_condition(self):
        # Even with every task outside the note done (the W4.5 case).
        self.assertEqual(wiki.readiness(self.tasks["X1.7"], self.tasks), "check")

    def test_missing_depends_field_needs_a_person(self):
        self.assertEqual(wiki.readiness(self.tasks["X1.8"], self.tasks), "check")

    def depends(self, text):
        return wiki.readiness(wiki.Task("Z9.9", "z", "todo", "todo", None, "", "", "doc", text), self.tasks)

    def test_conditional_note_without_a_task_id(self):
        self.assertEqual(self.depends("X0.2 (if the owner says build)"), "check")

    def test_or_that_can_already_hold_needs_a_person(self):
        # The C5.1 case: "C1.8, and A3.1 or A4.1" with C1.8 and A3.1 done, A4.1 not.
        self.assertEqual(self.depends("X0.1, and X0.2 or X1.2"), "check")

    def test_or_spellings_that_can_already_hold(self):
        self.assertEqual(self.depends("X0.1, and X0.2, or X1.2"), "check")
        self.assertEqual(self.depends("X0.1 and X1.2 OR X0.2"), "check")
        self.assertEqual(self.depends("X1.2 or owner sign-off"), "check")

    def test_parenthesised_task_is_a_dependency_not_a_note(self):
        self.assertEqual(self.depends("X1.1 or (X1.2)"), "waiting")
        self.assertEqual(self.depends("X1.1 or (X0.1)"), "check")

    def test_or_that_cannot_hold_yet_is_waiting(self):
        self.assertEqual(self.depends("X0.1, and X1.1 or X1.2"), "waiting")
        self.assertEqual(self.depends("X1.1 (and X0.1 if built)"), "waiting")

    def test_no_dependencies(self):
        self.assertEqual(wiki.readiness(self.tasks["X0.1"], self.tasks), "ready")


class Build(unittest.TestCase):
    def write_repo(self, directory: Path, order):
        files = {
            "canvas-roadmap.md": ROADMAP,
            "wiring-roadmap.md": OTHER,
            "canvas-plan.md": "# Plan\n\n## 1. Scope\n\n### D1 Thing\n\n```\n## not a heading\n```\n",
            "docs/measurements/X1.10.md": "# X1.10 later\n",
            "docs/measurements/X1.2.md": "# X1.2 earlier\n",
            # Equal natural keys: the file name must break the tie, not directory order.
            "docs/measurements/T2.7.md": "# T2.7\n",
            "docs/measurements/T02.7.md": "# T02.7\n",
        }
        for name in order(list(files)):
            path = directory / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(files[name], encoding="utf-8")

    def build(self, order=lambda names: names):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self.write_repo(root, order)
            return wiki.build(root, "owner/name", "0123456789abcdef")

    def test_deterministic_whatever_order_files_were_written(self):
        self.assertEqual(self.build(), self.build(lambda names: list(reversed(names))))

    def test_pages(self):
        pages = self.build()
        self.assertEqual(sorted(pages), [
            "Canvas-roadmap.md", "Home.md", "Human-checkpoints.md", "Log.md", "Measurements.md",
            "Plans.md", "Ready-to-start.md", "Wiring-roadmap.md", "_Footer.md", "_Sidebar.md",
        ])
        home = pages["Home.md"]
        self.assertIn("| [Canvas](Canvas-roadmap) | 2 | 0 | 6 | 0 | 1 | Milestone X1: work |", home)
        self.assertIn("## Ready to start (1)", home)  # X1.1 only
        self.assertIn("Do not edit this wiki", home)

    def test_log_is_newest_first_across_roadmaps(self):
        log = self.build()["Log.md"]
        order = re.findall(r"\*\*(\d{4}-\d{2}-\d{2})\*\* \((\w+)\) (\S+)", log)
        self.assertEqual(order, [
            ("2026-11-22", "Wiring", "Y1.1"),
            ("2026-11-21", "Canvas", "X1.2"),
            ("2026-11-21", "Canvas", "X0.2"),
            ("2026-11-21", "Wiring", "Y1.1"),
            ("2026-11-20", "Canvas", "X0.1"),
        ])

    def test_measurements_sort_naturally_and_plans_skip_fenced_headings(self):
        pages = self.build()
        self.assertLess(pages["Measurements.md"].index("X1.2 earlier"), pages["Measurements.md"].index("X1.10 later"))
        self.assertIn("1. Scope", pages["Plans.md"])
        self.assertNotIn("not a heading", pages["Plans.md"])

    def test_checkpoint_status_comes_from_the_task(self):
        page = self.build()["Human-checkpoints.md"]
        self.assertIn("X0.2 done → HX1 (signed 2026-11-20)", page)
        self.assertIn("X1.2 needs-human → HX2", page)

    def test_duplicate_task_ids_are_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "canvas-roadmap.md").write_text(ROADMAP, encoding="utf-8")
            (root / "wiring-roadmap.md").write_text(ROADMAP, encoding="utf-8")
            with self.assertRaises(ValueError):
                wiki.build(root, "owner/name", "0" * 40)

    def test_shorten_never_cuts_inside_code_or_links(self):
        self.assertEqual(wiki.shorten("one `two three` four", 12), "one …")
        self.assertEqual(wiki.shorten("one ``a ` b c`` four", 12), "one …")
        self.assertEqual(wiki.shorten("see [the plan](x.md) now", 14), "see …")
        self.assertEqual(wiki.shorten("short", 12), "short")
        self.assertEqual(wiki.shorten('see [x](docs.md "A (long) title") and more', 26), "see …")
        self.assertEqual(wiki.shorten("See [plan [draft with notes]](canvas-plan.md) now", 24), "See …")
        self.assertEqual(wiki.shorten(r"See [draft with \[notes](canvas-plan.md) now", 20), "See …")

    def test_relative_links_point_at_the_repository(self):
        links = wiki.Links("owner/name", "abc")
        self.assertEqual(
            links.absolute("[plan](wiring-plan.md), [web](https://x.y), [here](#top)"),
            "[plan](https://github.com/owner/name/blob/abc/wiring-plan.md), [web](https://x.y), [here](#top)",
        )
        self.assertEqual(links.absolute('[p](a.md "Title")'),
                         '[p](https://github.com/owner/name/blob/abc/a.md "Title")')
        self.assertEqual(links.absolute("write `[x](a.md)` like this"), "write `[x](a.md)` like this")
        self.assertEqual(links.absolute("[p](a.md 'T')"), "[p](https://github.com/owner/name/blob/abc/a.md 'T')")
        # A double-backtick span holding a triple run and a single backtick stays one span.
        literal = "``a ``` b ` [x](a.md) c`` then [y](b.md)"
        self.assertEqual(links.absolute(literal),
                         "``a ``` b ` [x](a.md) c`` then [y](https://github.com/owner/name/blob/abc/b.md)")
        self.assertEqual(links.absolute("[m](mailto:a@b.c) [s](ftp://x)"), "[m](mailto:a@b.c) [s](ftp://x)")
        # Formatted and nested labels.
        self.assertEqual(links.absolute("[`T2.7`](docs/m.md)"),
                         "[`T2.7`](https://github.com/owner/name/blob/abc/docs/m.md)")
        self.assertEqual(links.absolute("[a [b] c](x.md)"),
                         "[a [b] c](https://github.com/owner/name/blob/abc/x.md)")
        self.assertEqual(links.absolute(r"[draft with \[notes](p.md)"),
                         r"[draft with \[notes](https://github.com/owner/name/blob/abc/p.md)")
        # A link inside a link label: the inner one is the link.
        self.assertEqual(links.absolute("[quoted [plan](p.md)](x.md)"),
                         "[quoted [plan](https://github.com/owner/name/blob/abc/p.md)](x.md)")

    def test_escaped_pipes_stay_single_escaped_in_tables(self):
        self.assertIn("| HX4 | Either \\| both | — | Has an escaped pipe. |", self.build()["Human-checkpoints.md"])


class RealRepository(unittest.TestCase):
    """The repository's own roadmaps parse completely."""

    def test_every_task_heading_is_parsed(self):
        for path, page, name in wiki.ROADMAPS:
            text = (REPO_ROOT / path).read_text(encoding="utf-8")
            before_log = text.split("\n## Log", 1)[0]
            expected = re.findall(r"^### ([A-Z]{1,2}\d+\.\d+) .* · ", before_log, re.MULTILINE)
            parsed = wiki.parse_roadmap(0, path, page, name, text)
            checkpoint_rows = re.findall(
                r"^\| `?([A-Z]{1,2}\d+)`? \|", text.split("\n## Human checkpoints", 1)[-1].split("\n---", 1)[0],
                re.MULTILINE,
            ) if "\n## Human checkpoints" in text else []
            with self.subTest(path=path):
                self.assertEqual([t.id for t in parsed.tasks], expected)
                self.assertEqual(
                    [t.id for t in parsed.tasks if not t.lane and t.status != "other"], [],
                    "every live task has a Lane",
                )
                self.assertTrue(parsed.log, "the log has entries")
                self.assertTrue(all(e.date >= "2026-01-01" for e in parsed.log), "no wrapped line became an entry")
                self.assertEqual([c.id for c in parsed.checkpoints], checkpoint_rows)

    def test_regressions_from_review(self):
        sim = wiki.parse_roadmap(4, "sim-roadmap.md", "S", "S", (REPO_ROOT / "sim-roadmap.md").read_text(encoding="utf-8"))
        self.assertEqual([c.id for c in sim.checkpoints], ["HS1", "HS2", "HS3", "HS4"])
        mcp = wiki.parse_roadmap(2, "mcp-roadmap.md", "A", "A", (REPO_ROOT / "mcp-roadmap.md").read_text(encoding="utf-8"))
        self.assertTrue(any("2025-11-25 Tasks claim" in e.text for e in mcp.log))
        self.assertTrue(any(re.match(r"S0\.2\s+Re-review of #104", e.text) for e in sim.log),
                        "the double-spaced S0.2 re-review is its own entry")

    def test_builds(self):
        pages = wiki.build(REPO_ROOT, "owner/name", "0" * 40)
        self.assertIn("Home.md", pages)


if __name__ == "__main__":
    unittest.main()
