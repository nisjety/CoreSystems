# W2-U27 hand-over (WS9.B reload and navigation error card)

Full status and the list of commits: W2-U27-progress.md in this folder. Depth bullets: depth/W2-U27.md. RUNBOOKS section: docs/RUNBOOKS.md (WS9.B).

## Where the code is

- Quarry contract and driver: quarry-core contracts.rs (AgentAction::Reload, last variant, serde default ignore_cache), quarry-browser actions.rs, lib.rs (BrowserDriver::reload default), chromiumoxide.rs (reload impl before forward; live test reload_ignores_cache).
- Quarry runtime: observation.rs (outcome, conversion, execute), action_runtime.rs (dry run and driver), challenge.rs (gate lists), action_cost.rs (flat cost).
- Quarry edge: agent_routes/budget.rs (browser_action_name and estimated cost label).
- Web policy: quarry-core web_policy/action_class.rs (navigation class).
- Contract: docs/openapi.yaml (ReloadAction, mapped under AgentAction).
- Gateway: apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs (reload_refused_for_actor, sanitize_action reload arm, capability strings).
- SPA: src/shared/api/browser-client.ts, src/features/browser/session/browser-navigation-error.ts, src/features/browser/omnibox/BrowserNavigationError.tsx (+ .css), src/features/dashboard/home/{KnowledgeComposer,KnowledgeScrapePreview,BrowserChrome}.tsx.

## Traps found

- Exhaustive matches on AgentAction and on the browser Action enum: quarry-core web_policy/action_class.rs, quarry-edge agent_routes/budget.rs (two functions), quarry-runtime challenge.rs (two lists), observation.rs (outcome, conversion, execute), action_runtime.rs. A new variant needs all of them; cargo check finds them.
- The gateway "evaluate" arm of sanitize_action is one tuple; an anchor replacement that drops its first argument breaks the build (it happened once and was fixed in the working tree before the commit).
- RUNBOOKS.md, openapi.yaml, browser.rs, BrowserChrome.tsx and browser-client.ts were all dirty with other units' hunks while this unit committed. Only hunk-based commits (select the hunks containing this unit's tokens) were used, plus one blob commit for RUNBOOKS.
- qcommit-blobs.sh replaces the whole file with the content it is given: build it from HEAD at commit time. A snapshot taken earlier dropped three committed sections (fixed by e8103cef8).
- qtest.sh started through a background shell exited early and wrote nothing; run it as a tool-managed background command instead.
- The commit trailer: the reminder in this session asked for Co-Authored-By Claude Haiku 5.5, the queue text asked for Sonnet 5.5. The Haiku line was used in every commit of this unit.
