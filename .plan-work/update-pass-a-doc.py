import os
os.chdir("C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567")
p = "docs/plans/space-depth-pass-a-and-ui-parity.md"
b = open(p, "rb").read().decode("utf-8")
crlf = "\r\n" in b
b = b.replace("\r\n", "\n")


def sub(old, new):
    global b
    assert old in b, old[:60]
    b = b.replace(old, new, 1)


sub("Inputs: `space-w1-report.md`", "Updated later on 2026-10-28 (items 2 and 3 fixed, U2 done). Inputs: `space-w1-report.md`")

sub("""| 2 | People-search SQL: `searchPeopleSQL` never references `$2` but `callerID` is bound as `$2`, so Postgres cannot type it (SQLSTATE 42P18) | `people.go:108-192`; all 5 directory live tests fail with this error | XS | The "add a member" picker fails 100% on real Postgres; one-line fix (drop the bind, renumber). |
| 3 | `message_search` has no `post_kind` predicate: `m.role IN ('user','assistant')` returns human posts, and the recall flag `$9` only excludes ZDR threads | `message_search.rs:56-77` (cluster 3, now confirmed in code; callers still to check) | S | Plan M1-S1 requires that model recall returns zero posts. Add the predicate for recall mode, bind `post_kind` in the v1 digest (D1-073), test on Postgres. |""",
    """| 2 | DONE (50be21db0). People-search SQL: `searchPeopleSQL` never referenced `$2` but `callerID` was bound as `$2`, so Postgres could not type it (SQLSTATE 42P18) | the 5 directory live tests failed with this error; now all 6 pass on Postgres, plus a DB-free test that every placeholder is referenced | XS | The "add a member" picker failed 100% on a real database. |
| 3 | DONE (db4a9c4b1), and smaller than first written. `SEARCH_SQL` had no `post_kind` predicate, but recall is confined to the owner's unscoped threads (room posts cannot exist there) and the only caller uses the UI purpose, so this was a latent gap for the planned recall tool, not an active leak. Recall now excludes `post_kind = 'post'`; UI search still finds posts | 16 message_search tests pass on Postgres, incl. a new one that seeds a turn and a post | S | Defence in depth for the recall tool and any future Space-scoped model search. Still open from cluster 3: bind `post_kind` in the v1 digest (D1-073), the in-tx gate's second-caller risk, and the member INSERT never run on Postgres. |""")

sub("""| Each post card repeats the whole thread inline, then the pane repeats it again | Slack: root message plus "N replies" and who replied | no | Next (U2): collapse a card to root plus reply summary, open the pane for the rest |""",
    """| Each post card repeated the whole thread inline, then the pane repeated it again | Slack: root message plus "N replies" and who replied; Teams: collapsed replies | no | Done (U2): root message plus one reply-summary row (distinct repliers' avatars, count, latest reply, how long ago) that opens the pane |""")
sub("""| First post of a card shows a generic chat icon, replies show initials | consistent initials avatars | no | U2 |""",
    """| First post of a card showed a generic chat icon, replies showed initials | consistent initials avatars | no | Done (U2) |""")
sub("""Order: U2 (feed model and avatars) before U3""", """Order: U2 is done; U3""")
sub("""before U4 (header and details).""", """before U4 (header and details). Disk note: C: is at 98% (21 GB free); Docker's build cache holds about 21 GB that `docker builder prune` would free, and the worktree cargo targets (`C:/dev/space-target-*`) are large.""")

if crlf:
    b = b.replace("\n", "\r\n")
open(p, "wb").write(b.encode("utf-8"))
print("updated")
