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
// transcript and map the org's skills to the existing-skill baseline. A
// conversation-only thread, or one with nothing to render, comes back as a
// skip — and in both cases the org's skills are not even listed.
func (s *SessionCoreTranscriptSource) Fetch(
	ctx context.Context,
	ref SessionRef,
) (Transcript, error) {
	convo, err := s.client.ListConversation(ctx, &mpv1.ListConversationRequest{
		OrgId:    ref.OrgID,
		ThreadId: ref.ThreadID,
	})
	if err != nil {
		return Transcript{}, fmt.Errorf("sessionreview: list conversation for thread %s: %w", ref.ThreadID, err)
	}
	turns := activePath(convo.GetMessages())
	if conversationScoped(turns) {
		return Transcript{Skip: learning.SkipConversationScope}, nil
	}
	transcript := RenderTranscript(turns)
	if transcript == "" {
		return Transcript{Skip: learning.SkipEmptyTranscript}, nil
	}
	// As the run's owner: the baseline is the skills that person can already
	// see, their own proposals included, and no one else's private skills.
	skills, err := s.client.ListAgentSkills(ctx, &mpv1.ListAgentSkillsRequest{
		OrgId:       ref.OrgID,
		EnabledOnly: false,
		ForUserId:   ref.UserID,
	})
	if err != nil {
		return Transcript{}, fmt.Errorf("sessionreview: list agent skills for org %s: %w", ref.OrgID, err)
	}
	return Transcript{Text: transcript, Existing: toExistingSkills(skills.GetSkills())}, nil
}

// activePath drops the turns of a branch the thread's leaf is not on. Session Core
// answers only the active path to a default read, so this is a second guard: the
// reviewer never reads a turn it did not ask for. A turn is on another branch when it
// has siblings and is not on the path. A thread that was never versioned has no
// siblings, so the proto3 default of on_active_path (false) never drops its turns.
func activePath(msgs []*mpv1.SessionMessage) []*mpv1.SessionMessage {
	out := make([]*mpv1.SessionMessage, 0, len(msgs))
	for _, m := range msgs {
		if m.GetSiblingCount() > 0 && !m.GetOnActivePath() {
			continue
		}
		out = append(out, m)
	}
	return out
}

// conversationScoped reports whether any turn was written in conversation-only
// scope. Such a thread is kept out of workspace learning, so one such turn is
// enough to keep the whole thread out of review.
func conversationScoped(msgs []*mpv1.SessionMessage) bool {
	for _, m := range msgs {
		if m.GetMetadata().GetFields()["source_scope"].GetStringValue() == "conversation" {
			return true
		}
	}
	return false
}

// RenderTranscript renders conversation turns as a readable transcript for the
// skill-extraction reviewer: "ROLE: content" blocks in order. Best-judgment
// format — refine if review quality calls for richer rendering (e.g. tool
// call/result framing). A conversation-scoped thread renders empty.
func RenderTranscript(msgs []*mpv1.SessionMessage) string {
	if conversationScoped(msgs) {
		return ""
	}
	var b strings.Builder
	for _, m := range msgs {
		// A deleted message (WS12-S4) has no content left and must not teach the reviewer
		// anything; its tombstone is not part of the conversation either.
		if m.GetRevisionState() == "deleted" {
			continue
		}
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
