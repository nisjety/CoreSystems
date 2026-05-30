package schedule

const idemPrefix = "fbc1d94e94d756ede12c527b3d59e2204f58a623e6bd5a3d679eb03d93f22637"

// catalog is the in-memory seed list of scheduled work owned by the
// capability-core scheduler. All entries are scoped to the "triodelab"
// organization and carry a deterministic idempotency key.
var catalog = Catalog{
	ScheduledWork: []ScheduledWork{
		{
			ID:             "schedule.registry.reload-nightly",
			IdempotencyKey: idemPrefix + ":schedule.registry.reload-nightly",
			OrgID:          "triodelab",
			Kind:           KindCron,
			CronExpr:       "0 3 * * *",
			NextFireAt:     "2025-01-02T03:00:00Z",
			Description:    "Nightly reload of the capability registry from its source.",
			Payload:        map[string]string{"target": "capabilityRegistry"},
		},
		{
			ID:             "schedule.memory.index-hourly",
			IdempotencyKey: idemPrefix + ":schedule.memory.index-hourly",
			OrgID:          "triodelab",
			Kind:           KindRecurring,
			CronExpr:       "0 * * * *",
			NextFireAt:     "2025-01-01T01:00:00Z",
			Description:    "Hourly indexing of queued memory writes.",
			Payload:        map[string]string{"queue": "memoryWriteQueue"},
		},
		{
			ID:             "schedule.trigger.webhook-ingest",
			IdempotencyKey: idemPrefix + ":schedule.trigger.webhook-ingest",
			OrgID:          "triodelab",
			Kind:           KindRemoteTrigger,
			Description:    "Externally triggered ingestion run (fires on remote webhook).",
			Payload:        map[string]string{"source": "ingestion-plane"},
		},
	},
}
