"""Add the cross-lock-group legs left by the W2 run wf_b0595886-47e and print assumeDone ids."""
import json, re, os

HERE = os.path.dirname(os.path.abspath(__file__))
p = os.path.join(HERE, "w2-build.js")
t = open(p, encoding="utf-8", newline="").read()
m = re.search(r"const UNITS = (\[.*?\n\])\n", t, re.S)
units = json.loads(m.group(1))
by = {u["id"]: u for u in units}


def leg(base, suf, group, what, deps):
    u = dict(by[base])
    u["id"] = base + "." + suf
    u["group"] = group
    u["what"] = what
    u["deps"] = deps
    u["level"] = by[base]["level"] + 1
    return u


def note(base, done):
    return f"LEG of {base} ({by[base]['what']}). {done} Read .plan-work/space-build/units/{base}.json (and any earlier leg unit files of {base}) first. Build ONLY: "


legs = [
    leg("WS11-S3", "gw", "gw", note("WS11-S3", "The Application half is committed (920bb2ac1).") +
        "the gateway BFF for attention: domains/spaces/attention.rs aggregate (attention list, item state, read-all, unread/mark-unread, level, follow, invite, reason on read), its routes, flags and compose entries, and the gateway dispatchers for the matching spaces.* actions. WS11-S4's SPA (907ee8498) already calls these routes: read units/WS11-S4.json and match its client paths and payloads exactly",
        ["WS11-S3"]),
    leg("WS11-S3", "spa", "spa-room", note("WS11-S3", "The Application half (920bb2ac1) and the gateway leg (units/WS11-S3.gw.json) are done.") +
        "the SPA action contracts in src/shared/actions for the attention actions the gw leg dispatches, and any wiring the WS11-S4 SPA (907ee8498) still lacks to reach them",
        ["WS11-S3.gw"]),
    leg("WS11-S10", "gw", "gw", note("WS11-S10", "The Application half is committed (c7584a983).") +
        "BFF GET/PUT /api/v1/spaces/status, roster status and agents activity routes, flag and compose entries, and the gateway dispatcher for spaces.set_status",
        ["WS11-S10"]),
    leg("WS11-S10", "spa", "spa-room", note("WS11-S10", "The Application half (c7584a983) and the gw leg (units/WS11-S10.gw.json) are done.") +
        "action contract spaces.set_status, spaces-client calls, SpaceStatusMenu.tsx and roster chips; a minimal AccountSettingsPage entry is allowed outside your group",
        ["WS11-S10.gw"]),
    leg("WS11-S11", "gw", "gw", note("WS11-S11", "The Application half is committed (f0860876e).") +
        "BFF reminder routes, flag space_reminders_api with compose entries, and the gateway dispatchers for spaces.create_reminder / spaces.cancel_reminder",
        ["WS11-S11"]),
    leg("WS11-S11", "spa", "spa-room", note("WS11-S11", "The Application half (f0860876e) and the gw leg (units/WS11-S11.gw.json) are done.") +
        "action contracts spaces.create_reminder / spaces.cancel_reminder, spaces-client calls and SpaceRemindMenu.tsx",
        ["WS11-S11.gw"]),
    leg("WS10-S7", "ctl", "ctl", note("WS10-S7", "The Session Core half is committed (5d3b6ad83).") +
        "the Control twin in user-core (approval_decide_decision.go and whatever the plan names for turn_origin on the Control side)",
        ["WS10-S7"]),
]
have = {u["id"] for u in units}
for lg in legs:
    if lg["id"] not in have:
        units.append(lg)
# Slices that depend on a half-built slice also wait for its legs.
legs_of = {}
for lg in legs:
    legs_of.setdefault(lg["id"].split(".")[0], []).append(lg["id"])
for u in units:
    if "." in u["id"]:
        continue
    for d in list(u["deps"]):
        for l in legs_of.get(d, []):
            if l not in u["deps"]:
                u["deps"].append(l)
t = t[:m.start(1)] + json.dumps(units, indent=1) + t[m.end(1):]
open(p, "w", encoding="utf-8", newline="\n").write(t)

done = []
for u in units:
    f = os.path.join(HERE, "units", u["id"] + ".json")
    if os.path.exists(f):
        s = (json.load(open(f, encoding="utf-8")).get("status") or "").lower()
        if s in ("built", "already-present", "partial"):
            done.append(u["id"])
todo = [u["id"] for u in units if u["id"] not in done]
print(len(units), "units;", len(done), "done;", len(todo), "to run:", todo)
json.dump(done, open(os.path.join(HERE, "assume-done-w2b.json"), "w", encoding="utf-8", newline="\n"))
