// Package skillsink is the concrete learning.Sink (matrix §G7): it persists
// accepted skill candidates into session-core's agent_skills via the
// SessionCore.UpsertAgentSkill RPC. The learning package stays pure (no gRPC);
// this adapter bridges it to the durable store.
package skillsink

import (
	"context"
	"fmt"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/capability-core/internal/learning"
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
// session-core. It is session-scoped: it carries the org_id (skill candidates
// don't), supplied when the learning loop runs for a given session.
type SessionCoreSink struct {
	client skillUpserter
	orgID  string
}

// NewSessionCoreSink builds a sink bound to one org. `client` is normally a
// generated mpv1.SessionCoreClient.
func NewSessionCoreSink(client skillUpserter, orgID string) *SessionCoreSink {
	return &SessionCoreSink{client: client, orgID: orgID}
}

// Persist writes each candidate via UpsertAgentSkill and returns the count
// actually written. A candidate suppressed by the server's provenance guard
// (`skipped_protected` — the target is a human-authored skill) is not counted
// but is not an error: the loop yields to the human, matching the DB-level
// guard and learning.SelectForPersistence.
func (s *SessionCoreSink) Persist(ctx context.Context, skills []learning.SkillCandidate) (int, error) {
	persisted := 0
	for i, sk := range skills {
		resp, err := s.client.UpsertAgentSkill(ctx, &mpv1.UpsertAgentSkillRequest{
			OrgId:           s.orgID,
			Name:            sk.Name,
			Description:     sk.Description,
			Content:         sk.Content,
			TriggerKeywords: sk.TriggerKeywords,
			Enabled:         true,
			Origin:          string(sk.Origin),
		})
		if err != nil {
			// By position, not name: the name is model output derived from the
			// conversation, and this error ends up in the consumer's logs.
			return persisted, fmt.Errorf("upsert skill %d of %d: %w", i+1, len(skills), err)
		}
		if resp != nil && !resp.GetSkippedProtected() {
			persisted++
		}
	}
	return persisted, nil
}
