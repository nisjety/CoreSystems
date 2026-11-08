"""Build batch-args/<ID>.launch.json for a Wave 3 batch from batch-args/<ID>.json (written by wave-args.py).

usage: python w3-launch.py W3-NN [extra note]
"""
import json, sys

D = r"C:\dev\CoresSystem\.claude\worktrees\epic-shaw-4d613b\.plan-work\execution"
bid = sys.argv[1]
extra = " ".join(sys.argv[2:])
a = json.load(open(rf"{D}\batch-args\{bid}.json", encoding="utf-8"))
EP_FILES = {"EP-4": "EP-4-inline-outputs-and-mcp-apps.md", "EP-12": "EP-12-sharing-export-and-agent-api.md", 
    "EP-1": "EP-1-browser-takeover.md", "EP-2": "EP-2-research-and-citations.md", "EP-3": "EP-3-artifact-canvas-and-composer.md",
    "EP-5": "EP-5-deck-pipeline.md", "EP-6": "EP-6-agents-and-automations-hubs.md", "EP-7": "EP-7-persistent-goals.md",
    "EP-8": "EP-8-scoped-memory-and-personalisation.md", "EP-9": "EP-9-policy-egress-and-compliance.md",
    "EP-10": "EP-10-connector-suggestions-and-write-confirmation.md", "EP-11": "EP-11-versioned-authored-behaviours.md",
    "EP-13": "EP-13-model-record-and-gemini.md", "EP-14": "EP-14-run-legibility-and-release-gates.md",
}
for s in a["slices"]:
    ep = "-".join(s["id"].split("-")[:2])
    s["notes"] = (s.get("notes", "") + f" Your design is docs/plans/wave3/{EP_FILES[ep]}: read its section 2 (verified state), section 3 (decisions; build on the recommendation where the row's decisions_defaults name it) and your slice's section in full before editing. The final migration and proto numbers are in docs/plans/wave3/00-integration-round-a.md and 00-integration-round-b.md (round B's session-core range 0090+ and its renumbering win over any number in the design body); if the number you need is not reserved there, take the next free one after both reports and say so in lead_todo.").strip()
NOTES = (
    " NOTE: run every pnpm command with the env var pnpm_config_verify_deps_before_run=false. Typecheck the frontend with node node_modules/typescript/bin/tsc6 -p tsconfig.app.json --noEmit (tsconfig.json checks nothing)."
    " NOTE: every new switch defaults off; never flip one. A change to any .proto needs generated code: put the exact buf/regen step in lead_todo instead of hand-editing generated Go files, keep the change additive, and comment every new field and message (buf lint ratchet)."
    " NOTE: one model-gateway plus execution-core test build takes most of the free disk, so build only the test targets your slice names; never create a new cargo target directory, use only the given targetDir."
    " NOTE: a change to a platform prompt or a tool definition must regenerate apps/Model Plane/rust/crates/mp-contracts/tests/fixtures/prompt-tool-manifest.json (MP_UPDATE_PROMPT_MANIFEST=1 cargo test -p mp-contracts --test prompt_tool_manifest) and add or update an eval case or replay cassette (scenario-touch gate)."
    " NOTE: Wave 3 is built in wave mode: build the code and its tests, log depth findings, no live stack and no docker (the lead runs docker). Live proofs go to the depth phase."
)
a["goal"] = a["goal"] + NOTES + (" " + extra if extra else "")
a["targetDir"] = "C:/dev/mp-target-w2"
a["argsFile"] = rf"C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/.plan-work/execution/batch-args/{bid}.json"
a["roles"] = {"implement": {"model": "haiku", "effort": "max"}, "test": {"model": "sonnet", "effort": "medium"}, "control": {"model": "opus", "effort": "medium"}}
json.dump(a, open(rf"{D}\batch-args\{bid}.json", "w", encoding="utf-8"), ensure_ascii=False)
json.dump(a, open(rf"{D}\batch-args\{bid}.launch.json", "w", encoding="utf-8"), ensure_ascii=False)
print(bid, [s["id"] for s in a["slices"]], len(json.dumps(a)))
