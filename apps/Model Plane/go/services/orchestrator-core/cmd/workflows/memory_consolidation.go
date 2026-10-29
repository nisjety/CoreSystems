package workflows

import (
	"fmt"
	"time"

	"go.temporal.io/sdk/temporal"
	"go.temporal.io/sdk/workflow"

	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/activities"
)

// MemoryConsolidationInput is the input for the memory consolidation workflow.
type MemoryConsolidationInput struct {
	OrgID    string `json:"org_id"`
	MaxItems int    `json:"max_items"`
}

// MemoryConsolidationOutput contains the results of memory consolidation.
type MemoryConsolidationOutput struct {
	EntriesProcessed    int    `json:"entries_processed"`
	EntriesConsolidated int    `json:"entries_consolidated"`
	Summary             string `json:"summary"`
}

// MemoryConsolidationWorkflow periodically consolidates episodic memory.
//
// Steps:
//  1. Query session-core for recent memory index entries.
//  2. Call inference-core to summarize/cluster the entries.
//  3. Write consolidated entries back to session-core.
func MemoryConsolidationWorkflow(ctx workflow.Context, input MemoryConsolidationInput) (MemoryConsolidationOutput, error) {
	logger := workflow.GetLogger(ctx)
	logger.Info("MemoryConsolidationWorkflow started", "org_id", input.OrgID)

	if input.MaxItems <= 0 {
		input.MaxItems = 100
	}

	activityOpts := workflow.ActivityOptions{
		StartToCloseTimeout: 3 * time.Minute,
		RetryPolicy: &temporal.RetryPolicy{
			InitialInterval:    time.Second,
			BackoffCoefficient: 2.0,
			MaximumAttempts:    3,
		},
	}
	actCtx := workflow.WithActivityOptions(ctx, activityOpts)

	// Step 1: Query recent memory entries from session-core.
	queryInput := activities.MemoryQueryInput{
		OrgID:    input.OrgID,
		Since:    workflow.Now(ctx).Add(-24 * time.Hour),
		MaxItems: input.MaxItems,
	}

	var entries []activities.MemoryEntry
	err := workflow.ExecuteActivity(actCtx,
		"QueryMemoryEntriesActivity",
		queryInput,
	).Get(ctx, &entries)
	if err != nil {
		return MemoryConsolidationOutput{}, fmt.Errorf("query memory entries: %w", err)
	}

	if len(entries) == 0 {
		logger.Info("no entries to consolidate", "org_id", input.OrgID)
		return MemoryConsolidationOutput{
			Summary: "no entries to consolidate",
		}, nil
	}

	// Step 2: Call inference-core to summarize/cluster.
	consolidationInput := activities.ConsolidationInput{
		Entries: entries,
	}

	var consolidated activities.ConsolidationOutput
	err = workflow.ExecuteActivity(actCtx,
		"SummarizeMemoryActivity",
		consolidationInput,
	).Get(ctx, &consolidated)
	if err != nil {
		return MemoryConsolidationOutput{}, fmt.Errorf("summarize memory: %w", err)
	}

	// Step 3: Write consolidated entries back.
	writeInput := activities.WriteMemoryInput{
		Entries: consolidated.ConsolidatedEntries,
	}

	err = workflow.ExecuteActivity(actCtx,
		"WriteConsolidatedMemoryActivity",
		writeInput,
	).Get(ctx, nil)
	if err != nil {
		return MemoryConsolidationOutput{}, fmt.Errorf("write consolidated memory: %w", err)
	}

	summary := fmt.Sprintf("consolidated %d entries into %d for org %s",
		len(entries), len(consolidated.ConsolidatedEntries), input.OrgID)

	logger.Info("MemoryConsolidationWorkflow completed",
		"org_id", input.OrgID,
		"processed", len(entries),
		"consolidated", len(consolidated.ConsolidatedEntries),
	)

	return MemoryConsolidationOutput{
		EntriesProcessed:    len(entries),
		EntriesConsolidated: len(consolidated.ConsolidatedEntries),
		Summary:             summary,
	}, nil
}
