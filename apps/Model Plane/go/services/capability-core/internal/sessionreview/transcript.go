package sessionreview

import (
	"context"
	"fmt"
	"strings"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/capability-core/internal/learning"
	"google.golang.org/grpc"
)

// sessionCoreReader is the slice of the session-core client the transcript
// source needs (interface segregation — the generated SessionCoreClient
// satisfies it, and tests inject a fake). ListConversation supplies the
// transcript; ListAgentSkills supplies the org's existing skills (the
// dedup/provenance baseline RunReview applies).
type sessionCoreReader interface {
	ListConversation(ctx context.Context, in *mpv1.ListConversationRequest, opts ...grpc.CallOption) (*mpv1.ListConversationResponse, error)
	ListAgentSkills(ctx context.Context, in *mpv1.ListAgentSkillsRequest, opts ...grpc.CallOption) (*mpv1.ListAgentSkillsResponse, error)
}

// SessionCoreTranscriptSource is the concrete [TranscriptSource] backed by
// session-core: it renders a run's thread conversation as the review transcript
// and lists the org's existing skills. Both RPCs are org-scoped server-side, so
// this can't read another org's data.
type SessionCoreTranscriptSource struct {
	client sessionCoreReader
}

// NewSessionCoreTranscriptSource builds the source over a session-core client
// (normally a generated mpv1.SessionCoreClient).
func NewSessionCoreTranscriptSource(client sessionCoreReader) *SessionCoreTranscriptSource {
	return &SessionCoreTranscriptSource{client: client}
}

// Fetch implements [TranscriptSource]: render the thread's conversation as the
// transcript and map the org's skills to the existing-skill baseline.
func (s *SessionCoreTranscriptSource) Fetch(
	ctx context.Context,
	ref SessionRef,
) (string, []learning.ExistingSkill, error) {
	convo, err := s.client.ListConversation(ctx, &mpv1.ListConversationRequest{
		OrgId:    ref.OrgID,
		ThreadId: ref.ThreadID,
	})
	if err != nil {
		return "", nil, fmt.Errorf("sessionreview: list conversation for thread %s: %w", ref.ThreadID, err)
	}
	transcript := RenderTranscript(convo.GetMessages())
	if transcript == "" {
		return "", nil, nil
	}
	skills, err := s.client.ListAgentSkills(ctx, &mpv1.ListAgentSkillsRequest{
		OrgId:       ref.OrgID,
		EnabledOnly: false,
	})
	if err != nil {
		return "", nil, fmt.Errorf("sessionreview: list agent skills for org %s: %w", ref.OrgID, err)
	}
	return transcript, toExistingSkills(skills.GetSkills()), nil
}

// RenderTranscript renders conversation turns as a readable transcript for the
// skill-extraction reviewer: "ROLE: content" blocks in order. Best-judgment
// format — refine if review quality calls for richer rendering (e.g. tool
// call/result framing).
func RenderTranscript(msgs []*mpv1.SessionMessage) string {
	for _, m := range msgs {
		if m.GetMetadata().GetFields()["source_scope"].GetStringValue() == "conversation" {
			return ""
		}
	}
	var b strings.Builder
	for _, m := range msgs {
		b.WriteString(strings.ToUpper(m.GetRole()))
		b.WriteString(": ")
		b.WriteString(m.GetContent())
		b.WriteString("\n\n")
	}
	return strings.TrimSpace(b.String())
}

// toExistingSkills maps session-core AgentSkills to the learning baseline. Only
// Name + Origin matter for RunReview's provenance/dedup guard.
func toExistingSkills(skills []*mpv1.AgentSkill) []learning.ExistingSkill {
	out := make([]learning.ExistingSkill, 0, len(skills))
	for _, sk := range skills {
		out = append(out, learning.ExistingSkill{
			Name:   sk.GetName(),
			Origin: learning.Origin(sk.GetOrigin()),
		})
	}
	return out
}
