import json, re

ROOT = "C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/"
SCRATCH = "C:/Users/IMAFER~1/AppData/Local/Temp/claude/C--dev-CoresSystem--claude-worktrees-practical-mccarthy-1dc567/98a6fa7c-4507-4864-9c20-d81fc3ffb9de/scratchpad/"
plan = open(ROOT + "docs/plans/space-hybrid-build-plan-2026-10-14.md", encoding="utf-8").read().split("\n")
heads = [(i + 1, l) for i, l in enumerate(plan) if re.match(r"^#{3,4} ", l)]
ws_range = {}
for idx, (ln, l) in enumerate(heads):
    m = re.match(r"^#### (WS\d\d) ", l)
    if m:
        ws_range[m.group(1)] = (ln, heads[idx + 1][0] - 1 if idx + 1 < len(heads) else len(plan))

def notes(sid):
    return json.load(open(ROOT + ".plan-work/space-build/units/%s.json" % sid, encoding="utf-8")).get("notes", "")

LEGS = [
    ("WS04-S2.gw", "gateway leg of WS04-S2 (room details: rename and topic): room_admin.rs PATCH route, flag space_room_details_ui, actions dispatcher entry; build on the Application leg already committed (read units/WS04-S2.json)", "L7", "gw", "WS04-S2"),
    ("WS04-S3.gw", "gateway leg of WS04-S3 (people directory search): directory.rs route, flag, limiter; the Control endpoint GET /api/v1/internal/directory/people is already committed (read units/WS04-S3.json)", "L7", "gw", "WS04-S3"),
    ("WS07-S9.gw", "gateway leg of WS07-S9 (room artifacts aggregate): room_artifacts.rs BFF route, flag SPACE_ROOM_ARTIFACTS_API, retention and shared_read fields; the Model Plane half (proto RPC, session-core handler, model-gateway route) is already committed (read units/WS07-S9.json)", "L7", "gw", "WS07-S9"),
    ("WS04-S2.spa", "SPA leg of WS04-S2: ws04.ts action contract, spaces-client method, errors.ts entries, SpaceRoomSettings.tsx, SpacePage topic header; depends on the gateway leg WS04-S2.gw (contract-first: use the route shape from the plan and the gateway leg's notes)", "L8", "spa-room", "WS04-S2"),
    ("WS04-S3.spa", "SPA leg of WS04-S3: spaces-client searchDirectoryPeople and the SpaceMemberControls search box; depends on WS04-S3.gw (use the route shape from the plan and the gateway leg's notes)", "L8", "spa-room", "WS04-S3"),
    ("WS07-S9.spa", "SPA leg of WS07-S9: getSpaceArtifacts in spaces-client and the room Artifacts list wiring that the slice's file list names for the SPA; depends on WS07-S9.gw", "L8", "spa-room", "WS07-S9"),
    ("WS07-S9.regen", "Go and Python proto regeneration for the WS07-S9 space_room.proto / sessions.proto additions (buf generate recipe in the memory file named in the build rules); commit only generated files under apps/Model Plane/go/gen and any python gen dir that is checked in", "L1", "sc", "WS07-S9"),
]
units = []
for lvl, (uid, what, lane, group, parent) in enumerate(LEGS):
    ws = parent[:4]
    rng = ws_range.get(ws)
    units.append({"id": uid, "what": what + ". Parent unit notes: " + notes(parent)[:600].replace("\n", " "),
                  "lane": lane, "group": group, "size": "S/M", "level": 0 if ".gw" in uid or ".regen" in uid else 1,
                  "deps": [d for d in (["WS04-S2.gw"] if uid == "WS04-S2.spa" else ["WS04-S3.gw"] if uid == "WS04-S3.spa" else ["WS07-S9.gw"] if uid == "WS07-S9.spa" else [])],
                  "ws": ws, "planStart": rng[0], "planEnd": rng[1]})
tpl = open(ROOT + ".plan-work/space-build/w1-template.js", encoding="utf-8").read()
tpl = tpl.replace("space-w1-build", "space-w1-legs").replace("Build wave W1 (M0 + M1) of the approved Space plan unit by unit", "Close the unbuilt legs of W1 partial slices (gateway, SPA, regen)")
open(SCRATCH + "w1-legs.js", "w", encoding="utf-8", newline="\n").write(tpl.replace("__UNITS__", json.dumps(units, indent=1)))
print(len(units), "leg units")
