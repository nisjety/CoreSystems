package sessionreview

import (
	"context"
	"errors"
	"strings"
	"testing"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/capability-core/internal/learning"
	"google.golang.org/grpc"
)

type fakeReader struct {
	convo     *mpv1.ListConversationResponse
	skills    *mpv1.ListAgentSkillsResponse
	convoErr  error
	skillsErr error
}

func (f *fakeReader) ListConversation(
	_ context.Context, _ *mpv1.ListConversationRequest, _ ...grpc.CallOption,
) (*mpv1.ListConversationResponse, error) {
	return f.convo, f.convoErr
}

func (f *fakeReader) ListAgentSkills(
	_ context.Context, _ *mpv1.ListAgentSkillsRequest, _ ...grpc.CallOption,
) (*mpv1.ListAgentSkillsResponse, error) {
	return f.skills, f.skillsErr
}

func TestRenderTranscript_OrdersAndLabelsTurns(t *testing.T) {
	out := RenderTranscript([]*mpv1.SessionMessage{
		{Role: "user", Content: "how do I cache?"},
		{Role: "assistant", Content: "use the LRU"},
	})
	want := "USER: how do I cache?\n\nASSISTANT: use the LRU"
	if out != want {
		t.Fatalf("transcript mismatch:\n got: %q\nwant: %q", out, want)
	}
	if RenderTranscript(nil) != "" {
		t.Fatal("empty conversation should render empty")
	}
}

func TestToExistingSkills_MapsNameAndOrigin(t *testing.T) {
	got := toExistingSkills([]*mpv1.AgentSkill{
		{Name: "Runbook", Origin: "user"},
		{Name: "Cache", Origin: "background_review"},
	})
	if len(got) != 2 {
		t.Fatalf("want 2, got %d", len(got))
	}
	if got[0].Name != "Runbook" || got[0].Origin != learning.OriginUser {
		t.Fatalf("bad map[0]: %+v", got[0])
	}
	if got[1].Origin != learning.OriginBackgroundReview {
		t.Fatalf("bad map[1] origin: %+v", got[1])
	}
}

func TestFetch_AssemblesTranscriptAndExisting(t *testing.T) {
	r := &fakeReader{
		convo: &mpv1.ListConversationResponse{Messages: []*mpv1.SessionMessage{
			{Role: "user", Content: "hi"},
		}},
		skills: &mpv1.ListAgentSkillsResponse{Skills: []*mpv1.AgentSkill{
			{Name: "Runbook", Origin: "user"},
		}},
	}
	src := NewSessionCoreTranscriptSource(r)
	transcript, existing, err := src.Fetch(context.Background(),
		SessionRef{OrgID: "o", RunID: "r", ThreadID: "t"})
	if err != nil {
		t.Fatalf("Fetch: %v", err)
	}
	if !strings.Contains(transcript, "USER: hi") {
		t.Fatalf("transcript: %q", transcript)
	}
	if len(existing) != 1 || existing[0].Name != "Runbook" {
		t.Fatalf("existing: %+v", existing)
	}
}

func TestFetch_WrapsRpcErrors(t *testing.T) {
	src := NewSessionCoreTranscriptSource(&fakeReader{convoErr: errors.New("pg down")})
	if _, _, err := src.Fetch(context.Background(), SessionRef{}); err == nil ||
		!strings.Contains(err.Error(), "list conversation") {
		t.Fatalf("expected wrapped conversation error, got %v", err)
	}
	src2 := NewSessionCoreTranscriptSource(&fakeReader{
		convo:     &mpv1.ListConversationResponse{},
		skillsErr: errors.New("pg down"),
	})
	if _, _, err := src2.Fetch(context.Background(), SessionRef{}); err == nil ||
		!strings.Contains(err.Error(), "list agent skills") {
		t.Fatalf("expected wrapped skills error, got %v", err)
	}
}
