"""Write the slim Workflow args for tools/run-lean.js from a W3 launch file.
usage: python lean-launch.py W3-NN [--design EP-4,EP-12]
Reads batch-args/W3-NN.launch.json (written by w3-launch.py, which holds the full slice rows and the design notes),
adds each slice's queue status and decision ids, and writes batch-args/W3-NN.lean.json. The agents read their full
row from the launch file, so the inline args stay small.
"""
import json
import sys

EX = r"C:\dev\CoresSystem\.claude\worktrees\epic-shaw-4d613b\.plan-work\execution"
DESIGN_FILES = {
    "EP-4": ("docs/plans/wave3/EP-4-inline-outputs-and-mcp-apps.md", "F-t3-044 inline interactive outputs and F-t3-045 MCP Apps / elicitation forms (plan section '### EP-4 Inline outputs and MCP Apps'). First slices: bridge-less static widget render in the opaque-origin sandbox, then the narrow postMessage bridge, then elicitation forms reusing the WS14 pause, then MCP ui:// resources in the existing frame; the dedicated user-content origin is deferred (ADR stub only)."),
    "EP-12": ("docs/plans/wave3/EP-12-sharing-export-and-agent-api.md", "F-t3-046 sharing, replay, export and import and F-t3-064 the public versioned Agent API with origin-tagged runs (plan section '### EP-12 Sharing, export and the public Agent API'). First slices: origin and parent id on every run, the API-key to audience-bound bearer exchange contract with Control (auth-core), the public lifecycle surface over the existing model-gateway run routes with OpenAPI; export of non-ZDR threads; ZDR threads refuse share/replay/export; Channel Plane stays docs-only."),
}

batch = sys.argv[1]
designs = sys.argv[sys.argv.index("--design") + 1].split(",") if "--design" in sys.argv else []
launch_path = rf"{EX}\batch-args\{batch}.launch.json"
launch = json.load(open(launch_path, encoding="utf-8"))
q = {r["id"]: r for r in json.load(open(EX + r"\queue.json", encoding="utf-8"))}
slim = {
    "batchId": batch,
    "wave": 3,
    "goal": launch.get("goal", "")[:300],
    "argsFile": launch_path.replace("\\", "/"),
    "slices": [
        {
            "id": s["id"],
            "title": s["title"],
            "importance": s.get("importance", "sonnet"),
            "strict": bool(s.get("strict")),
            "risk": s.get("risk", []),
            "status": q.get(s["id"], {}).get("status", "todo"),
            "decisions": q.get(s["id"], {}).get("decisions", []),
        }
        for s in launch["slices"]
    ],
    "isolate": "--isolate" in sys.argv,
    "designs": [
        {"epic": e, "file": DESIGN_FILES[e][0], "rowsFile": rf"{EX}\w3-rows\{e}.json".replace("\\", "/"), "brief": DESIGN_FILES[e][1]}
        for e in designs
    ],
}
out = rf"{EX}\batch-args\{batch}.lean.json"
json.dump(slim, open(out, "w", encoding="utf-8"), ensure_ascii=False)
print(out, [s["id"] for s in slim["slices"]], [d["epic"] for d in slim["designs"]], len(json.dumps(slim)))
