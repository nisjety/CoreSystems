package tasks

const idemPrefix = "fbc1d94e94d756ede12c527b3d59e2204f58a623e6bd5a3d679eb03d93f22637"

// catalog is the in-memory seed list of tasks owned by the capability-core
// task registry. All entries are scoped to the "triodelab" organization and
// carry a deterministic idempotency key so the surface is safe to replay.
var catalog = Catalog{
	Tasks: []Task{
		{
			ID:             "task.bootstrap.capability-catalog",
			IdempotencyKey: idemPrefix + ":task.bootstrap.capability-catalog",
			OrgID:          "triodelab",
			Status:         StatusCompleted,
			CreatedAt:      "2025-01-01T00:00:00Z",
			UpdatedAt:      "2025-01-01T00:05:00Z",
			Description:    "Seed the in-memory capability catalog from the static source.",
			Outputs:        []string{"capabilityRegistry"},
		},
		{
			ID:             "task.schedule.cron-sweep",
			IdempotencyKey: idemPrefix + ":task.schedule.cron-sweep",
			OrgID:          "triodelab",
			ParentRunID:    "run.model-plane.bootstrap",
			Assignee:       "subagent.scheduler",
			Status:         StatusAssigned,
			CreatedAt:      "2025-01-01T00:10:00Z",
			UpdatedAt:      "2025-01-01T00:10:00Z",
			Description:    "Periodic cron sweep of scheduled work items.",
			Inputs:         []string{"scheduleCatalog"},
		},
		{
			ID:             "task.coordination.reconcile-parent",
			IdempotencyKey: idemPrefix + ":task.coordination.reconcile-parent",
			OrgID:          "triodelab",
			ParentRunID:    "run.model-plane.bootstrap",
			Assignee:       "subagent.coordinator",
			Status:         StatusBlocked,
			CreatedAt:      "2025-01-01T00:15:00Z",
			UpdatedAt:      "2025-01-01T00:20:00Z",
			Description:    "Reconcile subagent outputs back into the parent run transcript.",
			Inputs:         []string{"subagentTeam"},
		},
		{
			ID:             "task.memory.index-latest",
			IdempotencyKey: idemPrefix + ":task.memory.index-latest",
			OrgID:          "triodelab",
			Status:         StatusCreated,
			CreatedAt:      "2025-01-01T00:25:00Z",
			UpdatedAt:      "2025-01-01T00:25:00Z",
			Description:    "Index newly produced long-term memory entries.",
			Inputs:         []string{"memoryWriteQueue"},
		},
	},
}
