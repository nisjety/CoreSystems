import re, sys

ROOT = "C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/"
OUT = ROOT + "docs/plans/space-hybrid-build-plan-2026-10-14.md"

MARKERS = re.compile(r"^\s*<!--\s*(SECTION COMPLETE|COMPLETE|NEXT)\s*-->\s*$")


def read(rel):
    with open(ROOT + rel, encoding="utf-8", newline="") as f:
        return f.read().replace("\r\n", "\n").replace("\r", "\n")


def bump(text, n):
    """Add n '#' to every ATX heading outside code fences; drop completion markers."""
    out, fence = [], False
    for line in text.split("\n"):
        if MARKERS.match(line):
            continue
        if line.lstrip().startswith("```"):
            fence = not fence
            out.append(line)
            continue
        if not fence and re.match(r"^#{1,6} ", line):
            line = "#" * n + line
        out.append(line)
    return "\n".join(out).rstrip("\n") + "\n"


front = read(".plan-work/sections/front.md")
dec = bump(read(".plan-work/analysis/02-decisions.md"), 1)
ws = [bump(read(".plan-work/sections/03%s.md" % x), 1) for x in "abcde"]
epics = read(".plan-work/analysis/03z-epics.md")
epics = re.sub(r"^# 03z Later waves as epics \(section 3, part z\)", "# 3.Z: Later waves as epics (E01 to E12)", epics, count=1)
epics = bump(epics, 2)
seq = bump(read(".plan-work/analysis/04-sequencing.md"), 1)
xs = bump(read(".plan-work/analysis/05-cross-session.md"), 1)
ver = bump(read(".plan-work/analysis/06-verification.md"), 1)
closing = bump(read(".plan-work/sections/closing.md"), 0)
front = bump(front, 0)

sec3_intro = """## 3. Workstreams

Section 3 holds one subsection per workstream (WS01 to WS20, 3.A to 3.E) in build-wave order, then the epics for later waves (3.Z). Each workstream subsection lists: the audit items it resolves, owner plane and service, lane and milestone, dependencies and decisions, the verified starting point, slices (id, size, engineer-weeks, plane, flag, dependency), the file-level change list with path tags, API/proto/data-model/migration changes with the final numbers from section 4.6, flags and rollout, tests with commands, definition of done and evidence, risks, cross-session touchpoints and open questions. Slice ids (`WSnn-Sk`) are the ones used in sections 2, 4, 5, 6 and 7. Where a workstream text and a section-2/4/5 table disagree, sections 2, 4 and 5 decide (they were written after the designs and re-verified the sibling branches on 2026-10-20).

| Part | Workstreams | Theme |
|---|---|---|
| 3.A | WS01 to WS04 | Human posts and reliable send, realtime substrate, governance admin, room administration |
| 3.B | WS05, WS06, WS07, WS16 | Security fixes and enterprise identity, Model Plane safety, frontend quick wins and artifacts, UI baseline and sidebar |
| 3.C | WS08 to WS11 | Addressed wake-up and mentions, agent work visible to the room, approvals, notifications and attention |
| 3.D | WS12 to WS15 | DMs and message lifecycle, search and navigation, attachments and sharing, composer and timeline |
| 3.E | WS17 to WS20 | Personal agents and memory, agent self-API, room canvas, room-scoped agent browser |
| 3.Z | E01 to E12 | Later waves as epics (W3 recommended, W4, W5) with the index of all 96 audit items |

"""

parts = [front, sec3_intro] + ws + [epics]
# order: front (0,1) -> 2 decisions -> 3 -> 4 -> 5 -> 6 -> 7
body = front.rstrip("\n") + "\n\n" + dec + "\n" + sec3_intro + "\n" + "\n".join(ws) + "\n" + epics + "\n" + seq + "\n" + xs + "\n" + ver + "\n" + closing

# table of contents
def anchor(h):
    a = h.strip().lower()
    a = re.sub(r"[`*_]", "", a)
    a = re.sub(r"[^\w\s-]", "", a, flags=re.UNICODE)
    a = re.sub(r"\s", "-", a)
    return a

toc, fence = [], False
for line in body.split("\n"):
    if line.lstrip().startswith("```"):
        fence = not fence
        continue
    if fence:
        continue
    m = re.match(r"^(#{2,4}) (.+)$", line)
    if not m:
        continue
    lvl, title = len(m.group(1)), m.group(2)
    if lvl == 2 or (lvl == 3 and re.match(r"^(2\.\d|3\.[A-Z]|4\.\d|5\.\d|6\.\d|7\.\d|0\.\d|1\.\d)", title)) or (lvl == 4 and re.match(r"^WS\d\d ", title)):
        toc.append("  " * (lvl - 2) + "- [" + title + "](#" + anchor(title) + ")")

# insert TOC after the first two lines (title + file line)
lines = body.split("\n")
head_end = 0
for i, l in enumerate(lines):
    if l.startswith("## 0."):
        head_end = i
        break
fast = (
    "**Fast path for an approval decision (about 15 pages):** section 0 (how to read, state note), section 1 (goal, success criteria, scope, plan at a glance), 2.0 and 2.6 (the decisions that gate most work and what the user must answer), 4.1 (milestones and what M0 must decide), 5.4 (what to carry to the sibling sessions), 7.1 and 7.2 (open questions and the first three pull requests). Everything else is reference for the engineers who build.\n"
)
toc_block = "## Contents\n\n" + fast + "\n" + "\n".join(toc) + "\n\n"
lines[head_end:head_end] = toc_block.split("\n")
body = "\n".join(lines)

with open(OUT, "w", encoding="utf-8", newline="") as f:
    f.write(body)
print("wrote", OUT, len(body), "chars", body.count("\n"), "lines", len(toc), "toc entries")
