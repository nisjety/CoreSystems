// Package coordination documents the durable owner for team/subagent
// coordination — spawning subagents, inter-agent messaging, summarization,
// reconciliation back to the parent run, and attaching results. It is
// surfaced to the model plane as a read-only catalog; live orchestration is
// performed by downstream coordination services.
package coordination

// Member is a single subagent participating in a coordinated team.
type Member struct {
	ID   string `json:"id"`
	Role string `json:"role"`
}

// Message is an inter-subagent message within a team transcript.
type Message struct {
	From string `json:"from"`
	To   string `json:"to,omitempty"`
	Body string `json:"body"`
	At   string `json:"at"`
}

// SubagentTeam is a durable group of subagents coordinating under a parent
// run. The catalog describes teams as they are surfaced to operators; the
// live Spawn/Message/Summarize/Reconcile/AttachResult operations are handled
// by downstream services.
type SubagentTeam struct {
	ID             string    `json:"id"`
	IdempotencyKey string    `json:"idempotencyKey"`
	OrgID          string    `json:"orgId"`
	ParentRunID    string    `json:"parentRunId"`
	Members        []Member  `json:"members,omitempty"`
	Messages       []Message `json:"messages,omitempty"`
	Summary        string    `json:"summary,omitempty"`
	Results        []string  `json:"results,omitempty"`
	Description    string    `json:"description,omitempty"`
}

// Catalog is the read-only list of subagent teams documenting the durable
// owner for coordination.
type Catalog struct {
	Teams []SubagentTeam `json:"teams"`
}

// Load returns the seeded coordination catalog.
func Load() Catalog { return catalog }
