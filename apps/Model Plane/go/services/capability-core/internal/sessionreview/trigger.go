// Package sessionreview is the G7 learning-loop TRIGGER (matrix §G7): it turns
// a run-completion event into a skill-learning review. It is the event-driven
// counterpart to the rest of G7 — the reviewer (internal/llmreviewer), the sink
// (internal/skillsink), and the control flow (learning.RunReview) are built and
// tested; this package decides WHEN they run and assembles the call.
//
// # Design (best-judgment; the NATS/client plumbing is flagged unverified)
//
// Event-driven via NATS, NOT Temporal: capability-core has no Temporal
// dependency by design, and the platform is already event-sourced
// (RUN_COMPLETED on `mp.v1.run.*`). This mirrors the §4.3 reconcile-consumer
// pattern — capability-core subscribes to run completions and reviews the
// session. ([ParseRunCompleted] + [OnRunCompleted] are the decision/orchestration
// brain and are fully unit-tested here over injected interfaces.)
//
// What remains cross-service plumbing (build when wired to the running stack):
//   - the live NATS subscriber that feeds [ParseRunCompleted] (capability-core
//     has no subscriber today),
//   - the concrete [TranscriptSource] backed by session-core (ReplayThread for
//     the transcript + agent_skills for the existing-skill list),
//   - constructing the real reviewer/sink (inference-core + session-core gRPC
//     clients) at the call site.
//
// This package itself has NO unverified code: every dependency is an interface,
// so it compiles and is unit-tested with zero stack. It defines the exact
// contract that plumbing must satisfy.
package sessionreview

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"

	"github.com/triodelab/model-plane/pkg/envelope"
	"github.com/triodelab/model-plane/services/capability-core/internal/learning"
)

// RunCompletedEventType is the envelope `event_type` that triggers a review.
const RunCompletedEventType = "RUN_COMPLETED"

// SessionRef identifies the session whose transcript should be reviewed.
type SessionRef struct {
	OrgID    string
	RunID    string
	ThreadID string // best-effort, from the event payload when present
}

// ParseRunCompleted extracts a [SessionRef] from a RUN_COMPLETED envelope.
// Returns (_, false) for any other event type, an empty org, or a
// `resource_ref` that carries no run id. The run id comes from `resource_ref`
// ("run/<id>" per the envelope convention; "run:<id>" is also tolerated); the
// org from `org_id`; the thread id best-effort from `payload.thread_id`.
//
// NOTE (unverified): confirm the exact RUN_COMPLETED `resource_ref`/`payload`
// shape against a live event — this decode is intentionally tolerant.
func ParseRunCompleted(env *envelope.Envelope) (SessionRef, bool) {
	if env == nil || env.EventType != RunCompletedEventType || env.OrgID == "" {
		return SessionRef{}, false
	}
	runID := ""
	for _, prefix := range []string{"run/", "run:"} {
		if id, ok := strings.CutPrefix(env.ResourceRef, prefix); ok && id != "" {
			runID = id
			break
		}
	}
	if runID == "" {
		return SessionRef{}, false
	}
	ref := SessionRef{OrgID: env.OrgID, RunID: runID}
	if len(env.Payload) > 0 {
		var p struct {
			ThreadID string `json:"thread_id"`
		}
		_ = json.Unmarshal(env.Payload, &p) // best-effort; absence is fine
		ref.ThreadID = p.ThreadID
	}
	return ref, true
}

// TranscriptSource fetches what a review needs for a session: the transcript
// text and the org's already-registered skills (for the dedup hint). The
// concrete implementation is session-core-backed (ReplayThread + agent_skills)
// and is wired when the running stack is available.
type TranscriptSource interface {
	Fetch(ctx context.Context, ref SessionRef) (transcript string, existing []learning.ExistingSkill, err error)
}

// OnRunCompleted runs one learning review for a completed run: fetch the
// transcript + existing skills, then [learning.RunReview] (which applies the
// provenance/dedup/confidence policy and persists survivors via the sink).
// Returns the number of skills persisted. Every dependency is injected, so this
// orchestration is unit-tested with fakes — no stack required.
func OnRunCompleted(
	ctx context.Context,
	ref SessionRef,
	src TranscriptSource,
	reviewer learning.Reviewer,
	sink learning.Sink,
) (int, error) {
	if src == nil || reviewer == nil || sink == nil {
		return 0, fmt.Errorf("sessionreview: src, reviewer and sink are required")
	}
	transcript, existing, err := src.Fetch(ctx, ref)
	if err != nil {
		return 0, fmt.Errorf("sessionreview: fetch transcript for run %s: %w", ref.RunID, err)
	}
	return learning.RunReview(ctx, transcript, existing, reviewer, sink)
}
