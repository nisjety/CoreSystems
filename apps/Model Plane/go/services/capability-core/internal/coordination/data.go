package coordination

const idemPrefix = "fbc1d94e94d756ede12c527b3d59e2204f58a623e6bd5a3d679eb03d93f22637"

// catalog is the in-memory seed list of subagent teams owned by the
// capability-core coordinator. All entries are scoped to the "triodelab"
// organization and carry a deterministic idempotency key.
var catalog = Catalog{
	Teams: []SubagentTeam{
		{
			ID:             "team.research.deep-dive",
			IdempotencyKey: idemPrefix + ":team.research.deep-dive",
			OrgID:          "triodelab",
			ParentRunID:    "run.model-plane.bootstrap",
			Description:    "Deep-research team that fans out across web and corpora.",
			Members: []Member{
				{ID: "subagent.planner", Role: "planner"},
				{ID: "subagent.researcher", Role: "researcher"},
				{ID: "subagent.synthesizer", Role: "synthesizer"},
			},
			Messages: []Message{
				{From: "subagent.planner", To: "subagent.researcher", Body: "Enumerate sources for the query.", At: "2025-01-01T00:30:00Z"},
				{From: "subagent.researcher", To: "subagent.synthesizer", Body: "Sources enumerated; handing off findings.", At: "2025-01-01T00:35:00Z"},
			},
			Summary: "Three-agent pipeline: plan → research → synthesize.",
			Results: []string{"artifact.research.summary"},
		},
		{
			ID:             "team.build.refactor",
			IdempotencyKey: idemPrefix + ":team.build.refactor",
			OrgID:          "triodelab",
			ParentRunID:    "run.model-plane.bootstrap",
			Description:    "Parallel refactor team attaching patches back to the parent run.",
			Members: []Member{
				{ID: "subagent.scanner", Role: "scanner"},
				{ID: "subagent.patcher", Role: "patcher"},
			},
			Results: []string{"artifact.patch.set"},
		},
	},
}
