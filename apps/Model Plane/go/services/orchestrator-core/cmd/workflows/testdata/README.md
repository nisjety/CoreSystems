# Workflow Replay Test Fixtures

This directory holds exported Temporal workflow history JSON used by
`TestInteractiveRun_ReplayFromHistory` in `replay_test.go`.

The replay test verifies that the current workflow code remains
deterministically compatible with histories produced by previous versions —
a non-negotiable requirement for durable Temporal workflows.

## Expected fixtures

- `interactive_run_history.json` — one successful `InteractiveRunSupervision`
  execution covering Steps 1–4 (plan → initial nudge → reflect → final nudge)
  with the happy-path envelope stream.
- `approval_wait_history.json` — one `InteractiveRunSupervision` execution
  that entered the human-approval selector (step-loop returned
  `needs-human`), was persisted while blocked on the `approval` signal, and
  then resumed to completion after the signal was delivered. Proves durable
  cross-restart survival of the approval wait.

When a fixture is missing the corresponding replay test is **skipped**, not
failed, so the rest of the suite stays green in clean checkouts.

## Regenerating a fixture

Prerequisites: Temporal dev server (`temporal` CLI) and the orchestrator-core
worker build.

1. Start a local Temporal dev server:

   ```sh
   temporal server start-dev
   ```

2. In another shell, start the worker:

   ```sh
   cd apps/Model\ Plane/go/services/orchestrator-core
   go run ./cmd
   ```

3. Trigger an `InteractiveRunSupervision` execution with a stable workflow ID,
   e.g.:

   ```sh
   temporal workflow start \
     --type InteractiveRunSupervision \
     --task-queue orchestrator-core \
     --workflow-id replay-fixture-interactive-run \
     --input '{"runID":"run-happy-1","threadID":"thread-1","goal":"test goal","policy":"default","orgID":"org-happy","userID":"user-1"}'
   ```

4. Wait for completion, then export the history:

   ```sh
   temporal workflow show \
     --workflow-id replay-fixture-interactive-run \
     --output json \
     > cmd/workflows/testdata/interactive_run_history.json
   ```

5. Commit the regenerated file. Changes to workflow code that alter command
   ordering will break replay — that is the intended signal.

## Regenerating `approval_wait_history.json`

Prerequisites as above. This fixture requires a step-loop that returns
`needs-human` so the workflow enters the approval selector.

1. Start the dev server and worker as in steps 1–2 above, configuring the
   step-loop activity (or a test build tag) to emit a `needs-human` result
   on the first turn.

2. Trigger the workflow with a stable ID:

   ```sh
   temporal workflow start \
     --type InteractiveRunSupervision \
     --task-queue orchestrator-core \
     --workflow-id replay-fixture-approval-wait \
     --input '{"runID":"run-approve-fix-1","threadID":"thread-1","goal":"needs human","policy":"default","orgID":"org-approve","userID":"user-1"}'
   ```

3. Confirm the workflow is blocked in the approval selector (history will
   show `WorkflowTaskCompleted` followed by no further commands). Then send
   the approval signal:

   ```sh
   temporal workflow signal \
     --workflow-id replay-fixture-approval-wait \
     --name approval \
     --input '{}'
   ```

4. After completion, export the history:

   ```sh
   temporal workflow show \
     --workflow-id replay-fixture-approval-wait \
     --output json \
     > cmd/workflows/testdata/approval_wait_history.json
   ```

5. Commit. Replay of this fixture exercises the full suspend-and-resume
   durability path for the human-approval signal.

## Guidelines

- Keep fixtures small and deterministic; prefer short goals and fixed IDs.
- Never edit exported JSON by hand.
- When intentionally introducing a breaking workflow change, bump the workflow
  version via `workflow.GetVersion` and regenerate the fixture in the same PR.
