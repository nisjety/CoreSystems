// Package skillsink is the concrete learning.Sink (matrix §G7): it persists
// accepted skill candidates into session-core's agent_skills via the
// SessionCore.UpsertAgentSkill RPC. The learning package stays pure (no gRPC);
// this adapter bridges it to the durable store.
//
// A candidate is written as a proposal owned by the user whose run was
// reviewed: user-scoped, disabled and awaiting that user's decision. The sink
// asks for that shape and session-core enforces it; nothing here can write an
// enabled or org-wide skill.
package skillsink

import (
	"context"
	"fmt"
	"strings"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/capability-core/internal/learning"
	"github.com/triodelab/model-plane/services/capability-core/internal/telemetry"
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
	"google.golang.org/grpc"
)

// skillUpserter is the one method of SessionCoreClient this sink needs.
// Interface segregation keeps the sink unit-testable with a tiny fake and
// avoids depending on the full generated client surface.
type skillUpserter interface {
	UpsertAgentSkill(
		ctx context.Context,
		in *mpv1.UpsertAgentSkillRequest,
		opts ...grpc.CallOption,
	) (*mpv1.UpsertAgentSkillResponse, error)
}

// SessionCoreSink implements learning.Sink by writing each candidate to
// session-core. It is run-scoped: it carries the org, the owner and the run and
// thread the candidates were distilled from (skill candidates carry none of
// them), supplied when the learning loop runs for a given session.
type SessionCoreSink struct {
	client   skillUpserter
	orgID    string
	userID   string
	runID    string
	threadID string
}

// NewSessionCoreSink builds a sink bound to one run. `userID` is the owner of
// every proposal it writes. `client` is normally a generated
// mpv1.SessionCoreClient.
func NewSessionCoreSink(client skillUpserter, orgID, userID, runID, threadID string) *SessionCoreSink {
	return &SessionCoreSink{client: client, orgID: orgID, userID: userID, runID: runID, threadID: threadID}
}

// policySkips are the skip reasons that end the batch: they describe the owner
// or the organization, not the candidate, so every later candidate would be
// refused the same way.
var policySkips = map[string]bool{
	"memory_state":   true,
	"org_policy":     true,
	"zdr":            true,
	"owner_required": true,
}

// policyRefusalPrefix marks a skip session-core made under its content policy.
// It is about this candidate's own text, not the owner or the organization.
const policyRefusalPrefix = "skill_policy:"

// skillPolicyRefused counts the candidates session-core's content policy refused,
// labelled by the closed reason it returned. The candidate's text never reaches
// the meter.
var skillPolicyRefused, _ = otel.Meter(telemetry.MeterName).Int64Counter(
	"learning_skill_policy_refused_total",
	metric.WithDescription("Learned skill candidates refused by session-core's content policy, labelled by reason."),
)

// policyRefused records one refusal. It is a variable so a test can see each one.
var policyRefused = func(ctx context.Context, reason string) {
	skillPolicyRefused.Add(ctx, 1, metric.WithAttributes(attribute.String("reason", reason)))
}

// Persist writes each candidate via UpsertAgentSkill and returns the count
// actually written. A candidate the server did not write (`skip_reason`: the
// target is a human's skill, the owner already decided on it, or the owner's
// state or the organization's policy no longer allows learning) is not counted
// and is not an error: the loop yields, matching the DB-level guard and
// learning.SelectForPersistence. A refusal that is about the owner or the
// organization stops the batch. A candidate the content policy refused is
// counted by reason and skipped: it is not an error, so the delivery is not
// retried, and the rest of the batch is still written.
func (s *SessionCoreSink) Persist(ctx context.Context, skills []learning.SkillCandidate) (int, error) {
	persisted := 0
	for i, sk := range skills {
		resp, err := s.client.UpsertAgentSkill(ctx, &mpv1.UpsertAgentSkillRequest{
			OrgId:           s.orgID,
			Name:            sk.Name,
			Description:     sk.Description,
			Content:         sk.Content,
			TriggerKeywords: sk.TriggerKeywords,
			// What is asked for; session-core writes a disabled, user-scoped
			// proposal whatever these say.
			Enabled:        false,
			Origin:         string(sk.Origin),
			Scope:          "user",
			OwnerUserId:    s.userID,
			ReviewState:    "proposed",
			SourceRunId:    s.runID,
			SourceThreadId: s.threadID,
			ProposalReason: sk.Reason,
		})
		if err != nil {
			// By position, not name: the name is model output derived from the
			// conversation, and this error ends up in the consumer's logs.
			return persisted, fmt.Errorf("upsert skill %d of %d: %w", i+1, len(skills), err)
		}
		if resp == nil {
			continue
		}
		if resp.GetSkipReason() != "" || resp.GetSkippedProtected() {
			reason := resp.GetSkipReason()
			if strings.HasPrefix(reason, policyRefusalPrefix) {
				policyRefused(ctx, reason)
				continue
			}
			if policySkips[reason] {
				break
			}
			continue
		}
		persisted++
	}
	return persisted, nil
}
