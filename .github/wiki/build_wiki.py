"""Generate the GitHub wiki from the plans and roadmaps on `main`.

The wiki is a read-only view. The plans, roadmaps and `docs/measurements/` stay the
sources of truth (AGENTS.md); this script only reads them and writes Markdown pages.
`.github/workflows/wiki.yml` runs it on every push to `main` and pushes the result to
the wiki repository, replacing whatever was there.

Determinism: the same checkout gives byte-identical pages. Roadmaps are read in the
fixed order below, files are listed in sorted order, and ties between log entries of
the same date are broken by that roadmap order and then by position in the file.

Usage:
    python3 .github/wiki/build_wiki.py --root . --out wiki-out \
        --repo OWNER/NAME --sha COMMIT
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

# Roadmap file, wiki page name, display name. Order follows AGENTS.md.
ROADMAPS = (
    ("canvas-roadmap.md", "Canvas-roadmap", "Canvas"),
    ("wiring-roadmap.md", "Wiring-roadmap", "Wiring"),
    ("mcp-roadmap.md", "Agent-control-roadmap", "Agent control"),
    ("ui-roadmap.md", "Application-UI-roadmap", "Application UI"),
    ("sim-roadmap.md", "Simulation-roadmap", "Simulation"),
    ("parts-roadmap.md", "Parts-roadmap", "Parts"),
    ("context-roadmap.md", "Circuit-context-roadmap", "Circuit context"),
)

PLANS = (
    ("canvas-plan.md", "Canvas"),
    ("wiring-plan.md", "Wiring"),
    ("mcp-plan.md", "Agent control"),
    ("ui-plan.md", "Application UI"),
    ("sim-plan.md", "Simulation"),
    ("parts-plan.md", "Parts"),
    ("context-plan.md", "Circuit context"),
)

STATUSES = ("todo", "doing", "done", "blocked", "needs-human")
RECENT_LOG_ON_HOME = 10
HOME_LOG_CHARS = 200

TASK_HEADING = re.compile(r"^### (?P<id>[A-Z]{1,2}\d+\.\d+) (?P<title>.*?) · (?P<rest>.+)$")
TASK_ID = re.compile(r"\b[A-Z]{1,2}\d+\.\d+\b")
CHECKPOINT = re.compile(r"→\s*(?P<id>[A-Z]{1,2}\d+)")
# Two entry styles: "2026-11-20 X1.1 done — text" and "- **2026-11-20 · X1.1 · done:** text".
# The first word after the date must be a task or question id, or "plan", so a wrapped
# line that happens to start with a date (a protocol version, say) stays in its entry.
LOG_START = re.compile(r"^(?P<date>\d{4}-\d{2}-\d{2})[ \t]+(?P<text>(?:[A-Z]{1,3}\d+(?:\.\d+)?|plan)\b.*)$")
LOG_START_BULLET = re.compile(r"^- \*\*(?P<date>\d{4}-\d{2}-\d{2}) · (?P<head>.*?):?\*\*:? ?(?P<text>.*)$")
TABLE_ROW = re.compile(r"^\|(.+)\|\s*$")
UNESCAPED_PIPE = re.compile(r"(?<!\\)\|")
# A code span opens on a run of n backticks and closes on the next run of exactly n (GFM).
CODE_SPAN = re.compile(r"(?<!`)(?P<run>`+)(?!`).+?(?<!`)(?P=run)(?!`)", re.DOTALL)
# Words allowed in a Depends line besides task ids before it needs a human to read it.
DEPENDS_FILLER = re.compile(
    r"\b(?:and|parts|sim|context|ui|agent|canvas|wiring)\b|[,;.:\s]|`[^`]*`",
    re.IGNORECASE,
)
# What follows a link's "]": "(destination", an optional title, then ")".
LINK_TAIL = re.compile(
    r"\((?P<dest>[^)\s]+)(?P<title>\s+(?:\"[^\"]*\"|'[^']*'|\([^)]*\)))?\s*\)"
)
EXTERNAL = re.compile(r"^(?:[a-z][a-z0-9+.-]*:|#)", re.IGNORECASE)
# Words that make a Depends line or note conditional: only a person can judge those.
CONDITIONAL = re.compile(r"\b(?:if|unless|when|whether|once|until|or|optional)\b", re.IGNORECASE)
# An explicit "no dependencies"; a missing Depends field is not one.
NO_DEPENDS = {"none", "nothing", "—", "-"}


@dataclass
class Task:
    id: str
    title: str
    status: str  # one of STATUSES, or "other"
    status_text: str  # the heading text after " · "
    checkpoint: str | None
    milestone: str
    anchor: str
    lane: str = ""
    depends: str = ""


@dataclass
class LogEntry:
    date: str
    text: str
    roadmap: int  # index into ROADMAPS
    position: int


@dataclass
class Checkpoint:
    id: str
    when: str
    what: str


@dataclass
class Roadmap:
    index: int
    path: str
    page: str
    name: str
    tasks: list[Task] = field(default_factory=list)
    milestones: list[str] = field(default_factory=list)
    checkpoints: list[Checkpoint] = field(default_factory=list)
    log: list[LogEntry] = field(default_factory=list)


def github_anchor(heading: str, seen: dict[str, int]) -> str:
    """The id GitHub gives a Markdown heading, including the -1, -2 suffix for repeats.

    Follows GitHub's slugger: a taken slug gets the next free numeric suffix, and every
    emitted slug is reserved, so "Foo", "Foo", "Foo-1" give foo, foo-1, foo-1-1.
    """
    original = re.sub(r"[^\w\- ]", "", heading.strip().lower()).replace(" ", "-")
    slug = original
    while slug in seen:
        seen[original] += 1
        slug = f"{original}-{seen[original]}"
    seen[slug] = 0
    return slug


def parse_status(rest: str) -> tuple[str, str | None]:
    word = re.match(r"[a-z-]+", rest)
    status = word.group(0) if word and word.group(0) in STATUSES else "other"
    checkpoint = CHECKPOINT.search(rest)
    return status, checkpoint.group("id") if checkpoint else None


def bullet_field(bullet: str, name: str) -> str:
    """The value of `Name:` in a `- A: x · B: y` bullet, or ''."""
    for part in bullet.split(" · "):
        part = part.strip()
        if part.startswith(f"{name}:"):
            return part[len(name) + 1 :].strip()
    return ""


def parse_roadmap(index: int, path: str, page: str, name: str, text: str) -> Roadmap:
    roadmap = Roadmap(index, path, page, name)
    seen: dict[str, int] = {}
    section = ""
    task: Task | None = None
    bullet: list[str] | None = None
    in_fence = False
    in_log = False
    entry: list[str] | None = None

    def finish_bullet() -> None:
        nonlocal bullet
        if task is not None and bullet is not None:
            joined = " ".join(bullet)
            if "Lane:" in joined or "Depends:" in joined:
                task.lane = task.lane or bullet_field(joined, "Lane")
                task.depends = task.depends or bullet_field(joined, "Depends")
        bullet = None

    def finish_entry() -> None:
        nonlocal entry
        if entry:
            date, first = log_start(entry[0])
            body = " ".join([first, *(line.strip() for line in entry[1:])])
            roadmap.log.append(LogEntry(date, body, index, len(roadmap.log)))
        entry = None

    for line in text.splitlines():
        if line.startswith("```"):
            finish_entry()
            # Some logs keep their entries inside a fence; the template line in the
            # others never matches a dated entry, so fences do not hide log lines.
            if not in_log:
                in_fence = not in_fence
            continue
        if in_fence:
            continue
        if line.startswith("#"):
            finish_bullet()
            finish_entry()
            level = len(line) - len(line.lstrip("#"))
            heading = line[level:].strip()
            anchor = github_anchor(heading, seen)
            if level == 2:
                section = heading
                task = None
                in_log = heading == "Log"
                if heading.startswith("Milestone"):
                    roadmap.milestones.append(heading)
            elif level == 3:
                task = None
                match = TASK_HEADING.match(line)
                if match and not in_log:
                    status, checkpoint = parse_status(match.group("rest"))
                    task = Task(match.group("id"), match.group("title"), status,
                                match.group("rest"), checkpoint, section, anchor)
                    roadmap.tasks.append(task)
            continue
        if in_log:
            if log_start(line):
                finish_entry()
                entry = [line]
            elif entry is not None and line.strip():
                entry.append(line)
            elif not line.strip():
                finish_entry()
            continue
        if section == "Human checkpoints":
            row = TABLE_ROW.match(line)
            if row:
                cells = [c.strip() for c in UNESCAPED_PIPE.split(row.group(1))]
                ident = cells[0].strip("`")
                if len(cells) >= 3 and re.fullmatch(r"[A-Z]{1,2}\d+", ident):
                    roadmap.checkpoints.append(Checkpoint(ident, cells[1], " | ".join(cells[2:])))
            continue
        if task is not None:
            if line.startswith("- "):
                finish_bullet()
                bullet = [line[2:].strip()]
            elif bullet is not None and line.startswith("  ") and line.strip():
                bullet.append(line.strip())
            else:
                finish_bullet()
    finish_bullet()
    finish_entry()
    return roadmap


def log_start(line: str) -> tuple[str, str] | None:
    """(date, text) when a line starts a log entry, in either style."""
    match = LOG_START.match(line)
    if match:
        return match.group("date"), match.group("text")
    match = LOG_START_BULLET.match(line)
    if match:
        head = match.group("head").replace(" · ", " ").strip()
        return match.group("date"), f"{head} — {match.group('text')}"
    return None


def readiness(task: Task, tasks: dict[str, Task]) -> str:
    """'ready', 'waiting', or 'check' (the Depends line needs a person to read it)."""
    deps = task.depends.strip()
    if not deps:
        return "check"  # no Depends field at all
    if deps.lower() in NO_DEPENDS:
        return "ready"
    notes = re.findall(r"\([^)]*\)?", deps)  # "(why)" notes, even unclosed ones
    without_notes = re.sub(r"\([^)]*\)?", "", deps)
    ids = TASK_ID.findall(without_notes)
    if any(i not in tasks for i in ids):
        return "check"

    def done(i: str) -> bool:
        return tasks[i].status == "done"

    residue = DEPENDS_FILLER.sub("", TASK_ID.sub("", without_notes))
    # A task named in a note ("and W4.4 if built"), a conditional note ("if the owner
    # says build"), or wording such as "A or B" is for a person to judge.
    unclear = bool(residue) or not ids or any(
        TASK_ID.search(note) or CONDITIONAL.search(note) for note in notes
    )
    if not unclear:
        return "ready" if all(done(i) for i in ids) else "waiting"
    # Unclear lines: read clauses (split on , ; and) as all-of and " or " as any-of. A clause
    # none of whose alternatives can hold yet means waiting; otherwise a person decides.
    joined = re.sub(r"[,;]\s*(?=or\b)", " ", without_notes, flags=re.IGNORECASE)
    for clause in re.split(r"[,;]|\band\b", joined, flags=re.IGNORECASE):
        options = [TASK_ID.findall(alt) for alt in re.split(r"\bor\b", clause, flags=re.IGNORECASE)]
        if not all(options):
            continue  # an alternative that is not a task ("owner sign-off") may already hold
        if not any(all(done(i) for i in o) for o in options):
            return "waiting"
    return "check"


# --- Markdown scanning ---------------------------------------------------------------


@dataclass
class InlineLink:
    start: int
    end: int
    label: str  # "[...]", brackets included
    dest: str
    title: str  # leading whitespace included, or ""


def code_spans(text: str) -> list[tuple[int, int]]:
    return [m.span() for m in CODE_SPAN.finditer(text)]


def inline_links(text: str) -> list[InlineLink]:
    """Inline links outside code spans. Labels may hold code spans and nested brackets;
    titles may be "", '' or (). Link-like text wholly inside a code span is not a link."""
    spans = code_spans(text)
    span_at = {s: e for s, e in spans}

    def in_code(i: int) -> bool:
        return any(s <= i < e for s, e in spans)

    links, i = [], 0
    while i < len(text):
        if text[i] != "[" or in_code(i) or (i and text[i - 1] == "\\"):
            i += 1
            continue
        depth, j = 0, i
        while j < len(text):
            if j in span_at:
                j = span_at[j]
                continue
            if text[j] == "[":
                depth += 1
            elif text[j] == "]":
                depth -= 1
                if depth == 0:
                    break
            j += 1
        match = LINK_TAIL.match(text, j + 1) if j < len(text) else None
        if not match:
            i += 1
            continue
        links.append(InlineLink(i, match.end(), text[i : j + 1], match.group("dest"),
                                match.group("title") or ""))
        i = match.end()
    return links


# --- Rendering -----------------------------------------------------------------------


class Links:
    def __init__(self, repo: str, sha: str):
        self.base = f"https://github.com/{repo}/blob/{sha}"
        self.tree = f"https://github.com/{repo}/tree/{sha}"

    def file(self, path: str, anchor: str = "") -> str:
        return f"{self.base}/{path}" + (f"#{anchor}" if anchor else "")

    def absolute(self, text: str) -> str:
        """Point relative Markdown links (relative to the repository root) at the repository.

        Text inside code spans is left alone, so link syntax shown as an example survives.
        """
        out, last = [], 0
        for link in inline_links(text):
            if EXTERNAL.match(link.dest):
                continue
            out += [text[last : link.start], f"{link.label}({self.file(link.dest)}{link.title})"]
            last = link.end
        out.append(text[last:])
        return "".join(out)


def cell(text: str) -> str:
    """Escape pipes that are not already escaped, for a table cell."""
    return UNESCAPED_PIPE.sub(r"\\|", text).replace("\n", " ")


def status_label(task: Task) -> str:
    if task.status in ("blocked", "needs-human"):
        return f"**{cell(task.status_text)}**"
    return cell(task.status_text)


def task_link(task: Task, roadmap: Roadmap, links: Links) -> str:
    return f"[{task.id}]({links.file(roadmap.path, task.anchor)})"


def counts(tasks: list[Task]) -> dict[str, int]:
    result = {s: 0 for s in (*STATUSES, "other")}
    for task in tasks:
        result[task.status] += 1
    return result


def current_milestone(roadmap: Roadmap) -> str:
    for milestone in roadmap.milestones:
        if any(t.milestone == milestone and t.status != "done" for t in roadmap.tasks):
            return milestone
    return "all done" if roadmap.milestones else "—"


def render_log_entry(entry: LogEntry, roadmap_name: str = "", limit: int | None = None) -> str:
    where = f" ({roadmap_name})" if roadmap_name else ""
    return f"- **{entry.date}**{where} {shorten(entry.text, limit)}"


def shorten(text: str, limit: int | None) -> str:
    """Cut at a word boundary, but never inside a code span or a link."""
    if limit is None or len(text) <= limit:
        return text
    end = len(text[:limit].rsplit(" ", 1)[0])
    spans = code_spans(text) + [(link.start, link.end) for link in inline_links(text)]
    for start, stop in sorted(spans, reverse=True):
        if start < end < stop:
            end = start
    return text[:end].rstrip() + " …"


def all_log(roadmaps: list[Roadmap]) -> list[LogEntry]:
    entries = [e for r in roadmaps for e in r.log]
    # Newest first; within a date, later in roadmap order and file position first.
    return sorted(entries, key=lambda e: (e.date, -e.roadmap, e.position), reverse=True)


def page_home(roadmaps: list[Roadmap], links: Links, tasks: dict[str, tuple[Task, Roadmap]]) -> str:
    out = [
        "# Omnia",
        "",
        "A status view of the plans and roadmaps, generated from `main`. The files in the",
        f"repository are the source of truth: [ROADMAP.md]({links.file('ROADMAP.md')}) for",
        f"product stages and the `*-roadmap.md` files for tasks. See [AGENTS.md]({links.file('AGENTS.md')})",
        "for how work runs.",
        "",
        "> **Do not edit this wiki.** Every page is rebuilt on each push to `main` and hand",
        "> edits are overwritten. Change the roadmap or plan in a pull request instead.",
        "",
        "## Roadmaps",
        "",
        "| Roadmap | Done | Doing | To do | Blocked | Needs human | Current milestone |",
        "| --- | ---: | ---: | ---: | ---: | ---: | --- |",
    ]
    for roadmap in roadmaps:
        c = counts(roadmap.tasks)
        out.append(
            f"| [{roadmap.name}]({roadmap.page}) | {c['done']} | {c['doing']} | {c['todo']} "
            f"| {c['blocked']} | {c['needs-human']} | {cell(current_milestone(roadmap))} |"
        )
    out.append("")

    for status, heading in (("blocked", "Blocked"), ("needs-human", "Needs human"), ("doing", "In progress")):
        rows = [(t, r) for t, r in tasks.values() if t.status == status]
        out += [f"## {heading}", ""]
        if not rows:
            out += ["None.", ""]
            continue
        for task, roadmap in rows:
            out.append(f"- {task_link(task, roadmap, links)} {cell(task.title)} · {cell(task.status_text)}")
        out.append("")

    ready = [(t, r) for t, r in tasks.values() if t.status == "todo" and readiness(t, _plain(tasks)) == "ready"]
    out += [f"## Ready to start ({len(ready)})", "", "See [Ready to start](Ready-to-start).", ""]

    out += ["## Latest log entries", ""]
    out += [
        render_log_entry(e, roadmaps[e.roadmap].name, HOME_LOG_CHARS)
        for e in all_log(roadmaps)[:RECENT_LOG_ON_HOME]
    ]
    out += ["", "All entries: [Log](Log).", ""]
    return "\n".join(out)


def _plain(tasks: dict[str, tuple[Task, Roadmap]]) -> dict[str, Task]:
    return {k: v[0] for k, v in tasks.items()}


def page_roadmap(roadmap: Roadmap, links: Links, tasks: dict[str, tuple[Task, Roadmap]]) -> str:
    c = counts(roadmap.tasks)
    out = [
        f"# {roadmap.name} roadmap",
        "",
        f"Generated from [{roadmap.path}]({links.file(roadmap.path)}). Task ids link to the",
        "task in the roadmap file.",
        "",
        f"{c['done']} done, {c['doing']} doing, {c['todo']} to do, {c['blocked']} blocked, "
        f"{c['needs-human']} needing a human"
        + (f", {c['other']} other" if c["other"] else "")
        + ".",
        "",
    ]
    groups: dict[str, list[Task]] = {}
    for task in roadmap.tasks:
        groups.setdefault(task.milestone, []).append(task)
    for milestone, group in groups.items():
        out += [f"## {milestone}", "", "| Task | Title | Status | Lane | Depends |", "| --- | --- | --- | --- | --- |"]
        for task in group:
            out.append(
                f"| {task_link(task, roadmap, links)} | {cell(task.title)} | {status_label(task)} "
                f"| {cell(task.lane)} | {cell(link_ids(task.depends, tasks, links))} |"
            )
        out.append("")
    if roadmap.checkpoints:
        out += ["## Human checkpoints", "", f"See [Human checkpoints](Human-checkpoints).", ""]
    out += [f"## Log ({len(roadmap.log)} entries)", ""]
    out += [render_log_entry(e) for e in reversed(roadmap.log)]
    out.append("")
    return "\n".join(out)


def link_ids(text: str, tasks: dict[str, tuple[Task, Roadmap]], links: Links) -> str:
    def repl(match: re.Match[str]) -> str:
        found = tasks.get(match.group(0))
        if not found:
            return match.group(0)
        task, roadmap = found
        mark = " ✓" if task.status == "done" else ""
        return f"{task_link(task, roadmap, links)}{mark}"

    return TASK_ID.sub(repl, text) if text else "—"


def page_ready(roadmaps: list[Roadmap], links: Links, tasks: dict[str, tuple[Task, Roadmap]]) -> str:
    plain = _plain(tasks)
    out = [
        "# Ready to start",
        "",
        "`todo` tasks whose **Depends** line names only tasks that are `done` (a",
        "`needs-human` dependency is not done). This reads the Depends line only: also check",
        "the roadmap's rules, human checkpoints and owned files before starting, as",
        "AGENTS.md says.",
        "",
    ]
    for heading, kind, note in (
        ("Ready", "ready", None),
        ("Check by hand", "check",
         "The Depends line has wording a script should not judge (\"or\", a milestone, a "
         "condition), or names something that is not a task, and nothing in it is known to be unmet. "
         "Read the Depends line before starting."),
    ):
        out += [f"## {heading}", ""]
        if note:
            out += [note, ""]
        out += ["| Task | Roadmap | Title | Depends |", "| --- | --- | --- | --- |"]
        rows = 0
        for roadmap in roadmaps:
            for task in roadmap.tasks:
                if task.status == "todo" and readiness(task, plain) == kind:
                    rows += 1
                    out.append(
                        f"| {task_link(task, roadmap, links)} | [{roadmap.name}]({roadmap.page}) "
                        f"| {cell(task.title)} | {cell(link_ids(task.depends, tasks, links))} |"
                    )
        if rows == 0:
            out = out[:-2] + ["None."]
        out.append("")
    return "\n".join(out)


def page_checkpoints(roadmaps: list[Roadmap], links: Links, tasks: dict[str, tuple[Task, Roadmap]]) -> str:
    out = [
        "# Human checkpoints",
        "",
        "Agents stop at these (AGENTS.md hard rule 4). Status comes from the roadmap task",
        "whose heading carries `→ <checkpoint>`, or failing that the tasks named in **When**.",
        "",
    ]
    for roadmap in roadmaps:
        if not roadmap.checkpoints:
            continue
        out += [f"## {roadmap.name}", "", "| Checkpoint | When | Task status | What the owner does |",
                "| --- | --- | --- | --- |"]
        for cp in roadmap.checkpoints:
            owners = [t for t in roadmap.tasks if t.checkpoint == cp.id]
            if not owners:
                owners = [tasks[i][0] for i in TASK_ID.findall(cp.when) if i in tasks]
            status = "; ".join(f"{t.id} {t.status_text}" for t in owners) or "—"
            out.append(
                f"| {cp.id} | {cell(link_ids(cp.when, tasks, links))} | {cell(status)} | {cell(cp.what)} |"
            )
        out.append("")
    return "\n".join(out)


def page_log(roadmaps: list[Roadmap]) -> str:
    entries = all_log(roadmaps)
    out = ["# Log", "", f"Every roadmap log entry, newest first ({len(entries)} entries).", ""]
    current = None
    for entry in entries:
        month = entry.date[:7]
        if month != current:
            out += ["", f"## {month}", ""]
            current = month
        out.append(render_log_entry(entry, roadmaps[entry.roadmap].name))
    out.append("")
    return "\n".join(out)


def page_plans(root: Path, links: Links) -> str:
    out = [
        "# Plans",
        "",
        "The plans hold budgets, constants, decisions, invariants and open questions with",
        "their defaults. Each heading links into the file on `main`.",
        "",
        f"- [ROADMAP.md]({links.file('ROADMAP.md')}): product stages",
        f"- [AGENTS.md]({links.file('AGENTS.md')}): how work runs",
        "",
    ]
    for path, name in PLANS:
        file = root / path
        if not file.is_file():
            continue
        out += [f"## {name}: [{path}]({links.file(path)})", ""]
        seen: dict[str, int] = {}
        in_fence = False
        for line in file.read_text(encoding="utf-8").splitlines():
            if line.startswith("```"):
                in_fence = not in_fence
                continue
            if in_fence or not line.startswith("#"):
                continue
            level = len(line) - len(line.lstrip("#"))
            heading = line[level:].strip()
            anchor = github_anchor(heading, seen)
            if level in (2, 3):
                indent = "  " * (level - 2)
                out.append(f"{indent}- [{cell(heading)}]({links.file(path, anchor)})")
        out.append("")
    return "\n".join(out)


def page_measurements(root: Path, links: Links) -> str:
    out = [
        "# Measurements",
        "",
        "Reports in `docs/measurements/`. Only the reference machine certifies a budget",
        "(AGENTS.md hard rule 3); each report says which machine it ran on.",
        "",
        "| Report | Title |",
        "| --- | --- |",
    ]
    directory = root / "docs" / "measurements"
    for file in sorted(directory.glob("*.md"), key=lambda p: (natural_key(p.name), p.name)):
        title = file.stem
        for line in file.read_text(encoding="utf-8").splitlines():
            if line.startswith("# "):
                title = line[2:].strip()
                break
        rel = file.relative_to(root).as_posix()
        out.append(f"| [{file.stem}]({links.file(rel)}) | {cell(title)} |")
    out += ["", f"Raw data and scripts: [docs/measurements]({links.tree}/docs/measurements).", ""]
    return "\n".join(out)


def natural_key(name: str) -> list[object]:
    return [int(part) if part.isdigit() else part for part in re.split(r"(\d+)", name)]


def page_sidebar(roadmaps: list[Roadmap]) -> str:
    out = ["**[Home](Home)**", "", "**Roadmaps**", ""]
    out += [f"- [{r.name}]({r.page})" for r in roadmaps]
    out += ["", "**Views**", "", "- [Ready to start](Ready-to-start)",
            "- [Human checkpoints](Human-checkpoints)", "- [Log](Log)", "- [Plans](Plans)",
            "- [Measurements](Measurements)", ""]
    return "\n".join(out)


def page_footer(links: Links, sha: str) -> str:
    return (
        f"Generated from [`{sha[:12]}`]({links.tree}) by `.github/wiki/build_wiki.py`. "
        "Hand edits are overwritten.\n"
    )


def build(root: Path, repo: str, sha: str) -> dict[str, str]:
    """Return {page file name: Markdown} for the whole wiki."""
    links = Links(repo, sha)
    roadmaps = []
    for index, (path, page, name) in enumerate(ROADMAPS):
        file = root / path
        if file.is_file():
            roadmaps.append(parse_roadmap(index, path, page, name, file.read_text(encoding="utf-8")))
    tasks: dict[str, tuple[Task, Roadmap]] = {}
    for roadmap in roadmaps:
        for task in roadmap.tasks:
            if task.id in tasks:
                raise ValueError(f"task {task.id} appears twice ({roadmap.path})")
            tasks[task.id] = (task, roadmap)
        for entry in roadmap.log:
            entry.text = links.absolute(entry.text)
        for checkpoint in roadmap.checkpoints:
            checkpoint.when = links.absolute(checkpoint.when)
            checkpoint.what = links.absolute(checkpoint.what)

    pages = {
        "Home.md": page_home(roadmaps, links, tasks),
        "Ready-to-start.md": page_ready(roadmaps, links, tasks),
        "Human-checkpoints.md": page_checkpoints(roadmaps, links, tasks),
        "Log.md": page_log(roadmaps),
        "Plans.md": page_plans(root, links),
        "Measurements.md": page_measurements(root, links),
        "_Sidebar.md": page_sidebar(roadmaps),
        "_Footer.md": page_footer(links, sha),
    }
    for roadmap in roadmaps:
        pages[f"{roadmap.page}.md"] = page_roadmap(roadmap, links, tasks)
    return dict(sorted(pages.items()))


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=Path("."))
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--repo", required=True, help="OWNER/NAME")
    parser.add_argument("--sha", required=True)
    args = parser.parse_args(argv)
    args.out.mkdir(parents=True, exist_ok=True)
    for name, text in build(args.root, args.repo, args.sha).items():
        (args.out / name).write_text(text, encoding="utf-8", newline="\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
