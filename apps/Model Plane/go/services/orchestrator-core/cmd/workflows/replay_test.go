package workflows

import (
	"errors"
	"os"
	"testing"

	"github.com/stretchr/testify/require"
	"go.temporal.io/sdk/worker"
)

// TestInteractiveRun_ReplayFromHistory verifies that the current
// InteractiveRunSupervision workflow code remains deterministically
// compatible with a previously captured history.
//
// When the fixture is missing the test is skipped so clean checkouts stay
// green. See testdata/README.md for regeneration steps.
func TestInteractiveRun_ReplayFromHistory(t *testing.T) {
	const path = "testdata/interactive_run_history.json"

	if _, err := os.Stat(path); errors.Is(err, os.ErrNotExist) {
		t.Skipf("history fixture %q missing; regenerate via testdata/README.md", path)
	}

	replayer := worker.NewWorkflowReplayer()
	replayer.RegisterWorkflow(InteractiveRunSupervision)

	require.NoError(t, replayer.ReplayWorkflowHistoryFromJSONFile(nil, path))
}

// TestInteractiveRun_ApprovalDurability_ReplayFromHistory verifies that a
// workflow history captured while the workflow was suspended in the
// human-approval selector (and later resumed via the `approval` signal)
// still replays deterministically against the current workflow code.
//
// This closes the "Human approval wait is durable across restarts" gate:
// Temporal replay from a persisted history is the formal proof of
// cross-restart durability, because the worker process that replays the
// history is by construction a fresh process with no in-memory state.
//
// When the fixture is missing the test is skipped so clean checkouts stay
// green. See testdata/README.md for regeneration steps.
func TestInteractiveRun_ApprovalDurability_ReplayFromHistory(t *testing.T) {
	const path = "testdata/approval_wait_history.json"

	if _, err := os.Stat(path); errors.Is(err, os.ErrNotExist) {
		t.Skipf("history fixture %q missing; regenerate via testdata/README.md", path)
	}

	replayer := worker.NewWorkflowReplayer()
	replayer.RegisterWorkflow(InteractiveRunSupervision)

	require.NoError(t, replayer.ReplayWorkflowHistoryFromJSONFile(nil, path))
}
