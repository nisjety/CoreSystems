package sessionreview

import (
	"context"
	"encoding/json"
	"testing"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/envelope"
	"google.golang.org/grpc"
)

// fakeSessionClient satisfies reviewSessionClient (transcript reader + skill
// writer). fakeInfClient satisfies reviewInferenceClient.
type fakeSessionClient struct {
	convo   *mpv1.ListConversationResponse
	skills  *mpv1.ListAgentSkillsResponse
	upserts []*mpv1.UpsertAgentSkillRequest
}

func (f *fakeSessionClient) ListConversation(_ context.Context, _ *mpv1.ListConversationRequest, _ ...grpc.CallOption) (*mpv1.ListConversationResponse, error) {
	return f.convo, nil
}

func (f *fakeSessionClient) ListAgentSkills(_ context.Context, _ *mpv1.ListAgentSkillsRequest, _ ...grpc.CallOption) (*mpv1.ListAgentSkillsResponse, error) {
	return f.skills, nil
}

func (f *fakeSessionClient) UpsertAgentSkill(_ context.Context, in *mpv1.UpsertAgentSkillRequest, _ ...grpc.CallOption) (*mpv1.UpsertAgentSkillResponse, error) {
	f.upserts = append(f.upserts, in)
	return &mpv1.UpsertAgentSkillResponse{Created: true}, nil
}

type fakeInfClient struct{ content string }

func (f *fakeInfClient) Infer(_ context.Context, _ *mpv1.InferRequest, _ ...grpc.CallOption) (*mpv1.InferResponse, error) {
	return &mpv1.InferResponse{Content: f.content}, nil
}

func envBytes(t *testing.T, eventType, resourceRef, org, payload string) []byte {
	t.Helper()
	if payload == "" {
		payload = "{}" // empty RawMessage is invalid JSON
	}
	b, err := json.Marshal(&envelope.Envelope{
		EventType:   eventType,
		OrgID:       org,
		ResourceRef: resourceRef,
		Payload:     json.RawMessage(payload),
	})
	if err != nil {
		t.Fatalf("marshal envelope: %v", err)
	}
	return b
}

func TestRunCompletedSubjectIsLimitedToCanonicalRunEvents(t *testing.T) {
	if RunCompletedSubject != "mp.v1.run.*.event" {
		t.Fatalf("run-completed subscription = %q, want canonical run-event subject only", RunCompletedSubject)
	}
}

// The whole trigger path from a RUN_COMPLETED envelope to a persisted skill,
// faking only the session-core and inference-core boundaries.
func TestHandleRunCompleted_EndToEnd(t *testing.T) {
	sc := &fakeSessionClient{
		convo: &mpv1.ListConversationResponse{Messages: []*mpv1.SessionMessage{
			{Role: "user", Content: "how do I cache?"},
			{Role: "assistant", Content: "use an LRU"},
		}},
		skills: &mpv1.ListAgentSkillsResponse{}, // no existing skills
	}
	ic := &fakeInfClient{content: `{"skills":[{"name":"Caching","content":"use an LRU","confidence":0.9}]}`}

	data := envBytes(t, "RUN_COMPLETED", "run/r-1", "org-7", `{"thread_id":"t-1"}`)
	n, err := HandleRunCompleted(context.Background(), data, sc, ic, "test-model")
	if err != nil {
		t.Fatalf("HandleRunCompleted: %v", err)
	}
	if n != 1 {
		t.Fatalf("expected 1 skill persisted, got %d", n)
	}
	if len(sc.upserts) != 1 || sc.upserts[0].GetName() != "Caching" {
		t.Fatalf("unexpected upserts: %+v", sc.upserts)
	}
	// Provenance forced end-to-end.
	if sc.upserts[0].GetOrigin() != "background_review" {
		t.Fatalf("origin must be background_review, got %q", sc.upserts[0].GetOrigin())
	}
	if sc.upserts[0].GetOrgId() != "org-7" {
		t.Fatalf("org scoping lost: %q", sc.upserts[0].GetOrgId())
	}
}

func TestHandleRunCompleted_NonRunEventIgnored(t *testing.T) {
	data := envBytes(t, "STEP_COMPLETED", "run/r-1", "org-7", "")
	n, err := HandleRunCompleted(context.Background(), data,
		&fakeSessionClient{}, &fakeInfClient{}, "m")
	if err != nil || n != 0 {
		t.Fatalf("non-trigger event must yield 0/nil, got %d / %v", n, err)
	}
}

func TestHandleRunCompleted_BadEnvelopeErrors(t *testing.T) {
	_, err := HandleRunCompleted(context.Background(), []byte("not json"),
		&fakeSessionClient{}, &fakeInfClient{}, "m")
	if err == nil {
		t.Fatal("an undecodable envelope must error")
	}
}
