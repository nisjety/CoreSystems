package skillsink

import (
	"context"
	"errors"
	"testing"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/capability-core/internal/learning"
	"google.golang.org/grpc"
)

type fakeUpserter struct {
	reqs    []*mpv1.UpsertAgentSkillRequest
	skipped bool
	err     error
}

func (f *fakeUpserter) UpsertAgentSkill(
	_ context.Context,
	in *mpv1.UpsertAgentSkillRequest,
	_ ...grpc.CallOption,
) (*mpv1.UpsertAgentSkillResponse, error) {
	if f.err != nil {
		return nil, f.err
	}
	f.reqs = append(f.reqs, in)
	return &mpv1.UpsertAgentSkillResponse{
		Id:               "id-" + in.GetName(),
		Created:          true,
		SkippedProtected: f.skipped,
	}, nil
}

func TestPersist_MapsFieldsAndCountsWritten(t *testing.T) {
	f := &fakeUpserter{}
	sink := NewSessionCoreSink(f, "org1")
	n, err := sink.Persist(context.Background(), []learning.SkillCandidate{
		{
			Name:            "Cache Tips",
			Description:     "use the shared cache",
			Content:         "step 1...",
			TriggerKeywords: []string{"cache"},
			Origin:          learning.OriginBackgroundReview,
		},
	})
	if err != nil || n != 1 {
		t.Fatalf("expected 1 persisted, got n=%d err=%v", n, err)
	}
	if len(f.reqs) != 1 {
		t.Fatalf("expected 1 RPC, got %d", len(f.reqs))
	}
	r := f.reqs[0]
	if r.GetOrgId() != "org1" || r.GetName() != "Cache Tips" || !r.GetEnabled() {
		t.Fatalf("bad mapping: %+v", r)
	}
	if r.GetOrigin() != "background_review" {
		t.Fatalf("origin not mapped: %q", r.GetOrigin())
	}
	if len(r.GetTriggerKeywords()) != 1 || r.GetTriggerKeywords()[0] != "cache" {
		t.Fatalf("trigger keywords not mapped: %+v", r.GetTriggerKeywords())
	}
}

func TestPersist_SkippedProtectedNotCounted(t *testing.T) {
	f := &fakeUpserter{skipped: true}
	sink := NewSessionCoreSink(f, "org1")
	n, err := sink.Persist(context.Background(), []learning.SkillCandidate{
		{Name: "x", Content: "c"},
	})
	if err != nil || n != 0 {
		t.Fatalf("protected skip must persist 0 (no error), got n=%d err=%v", n, err)
	}
}

func TestPersist_RpcErrorPropagates(t *testing.T) {
	f := &fakeUpserter{err: errors.New("rpc down")}
	sink := NewSessionCoreSink(f, "org1")
	if _, err := sink.Persist(context.Background(), []learning.SkillCandidate{{Name: "x", Content: "c"}}); err == nil {
		t.Fatal("rpc error must propagate")
	}
}

// Compile-time assertion that SessionCoreSink satisfies learning.Sink.
var _ learning.Sink = (*SessionCoreSink)(nil)
