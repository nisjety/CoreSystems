# W2-U47 hand-over (WS9.F)

What was done: the framework-light browser session controller and its empty-preview adapter, plus one test file. Committed as 380360b38 in apps/Frontend Plane/verevonv3/src/features/browser/session/. Nothing else was edited. See W2-U47-progress.md for the list and commands, and depth/W2-U47.md for the depth bullets.

What is left (not in scope or blocked):
- expectedEpoch on setControlMode (WS9.I, after WS2.C is deployed).
- The wider test cases listed in depth (test phase at the end of the wave).
- The composer is not moved onto the controller (unscheduled cleanup).
- No UI entry point (WS9.G), so there is no evidence beyond mocked-client tests.

Traps found:
- Solid 2.0.0-rc.0 batches signal writes. A read right after a write outside a component returns the old value. `flush` from solid-js applies pending writes (the tests use it).
- `vi.resetAllMocks()` leaves `vi.fn()` returning undefined, so a mocked module used in a `.catch` chain (browser-loop's `controlBrowserAiRun`) must get a resolved value again in beforeEach.
- A factory-local function with the same name as an imported function shadows it (decideApproval). Alias the import.
- The verevonv3 vitest run of one jsdom file takes about 3 seconds once warm, and about 75 seconds on a cold start.
- The Co-Authored-By line is the Haiku 5.5 line from the session's attribution reminder. The workflow text asked for a Sonnet 5.5 line; the reminder was followed, so check the commit against the owner's convention.
