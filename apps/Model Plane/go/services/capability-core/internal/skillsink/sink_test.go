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
	// skipReasons answers the calls in order; a call past the end is written.
	skipReasons []string
	err         error
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
	reason := ""
	if len(f.reqs) <= len(f.skipReasons) {
		reason = f.skipReasons[len(f.reqs)-1]
	}
	return &mpv1.UpsertAgentSkillResponse{
		Id:               "id-" + in.GetName(),
		Created:          reason == "",
		SkippedProtected: f.skipped,
		SkipReason:       reason,
	}, nil
}

func TestPersist_MapsFieldsAndCountsWritten(t *testing.T) {
	f := &fakeUpserter{}
	sink := NewSessionCoreSink(f, "org1", "user-1", "run-1", "thread-1")
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
	if r.GetOrgId() != "org1" || r.GetName() != "Cache Tips" {
		t.Fatalf("bad mapping: %+v", r)
	}
	if r.GetOrigin() != "background_review" {
		t.Fatalf("origin not mapped: %q", r.GetOrigin())
	}
	if len(r.GetTriggerKeywords()) != 1 || r.GetTriggerKeywords()[0] != "cache" {
		t.Fatalf("trigger keywords not mapped: %+v", r.GetTriggerKeywords())
	}
}

// A learned skill is asked for as a disabled proposal owned by the run's user,
// with the run and thread it came from and the reviewer's reason. Session-core
// enforces the shape; the sink must never ask for anything wider.
func TestPersist_WritesDisabledUserScopedProposal(t *testing.T) {
	f := &fakeUpserter{}
	sink := NewSessionCoreSink(f, "org1", "user-1", "run-9", "thread-9")
	n, err := sink.Persist(context.Background(), []learning.SkillCandidate{
		{Name: "Weekly report", Content: "steps", Reason: "asked for three weeks running", Origin: learning.OriginBackgroundReview},
	})
	if err != nil || n != 1 {
		t.Fatalf("n=%d err=%v", n, err)
	}
	r := f.reqs[0]
	if r.GetEnabled() || r.GetScope() != "user" || r.GetOwnerUserId() != "user-1" || r.GetReviewState() != "proposed" {
		t.Fatalf("not a disabled user-scoped proposal: %+v", r)
	}
	if r.GetSourceRunId() != "run-9" || r.GetSourceThreadId() != "thread-9" || r.GetProposalReason() != "asked for three weeks running" {
		t.Fatalf("provenance not carried: %+v", r)
	}
}

// A candidate the server did not write is not counted and is not an error; a
// refusal about the owner or the organization ends the batch, since every later
// candidate would meet the same one.
func TestPersist_SkipReasonsAreNotCountedAndPolicyEndsTheBatch(t *testing.T) {
	candidates := []learning.SkillCandidate{{Name: "a", Content: "c"}, {Name: "b", Content: "c"}, {Name: "c", Content: "c"}}

	f := &fakeUpserter{skipReasons: []string{"accepted_exists", "", ""}}
	n, err := NewSessionCoreSink(f, "org1", "user-1", "run-1", "thread-1").Persist(context.Background(), candidates)
	if err != nil || n != 2 || len(f.reqs) != 3 {
		t.Fatalf("a candidate that already exists must be skipped and the rest written: n=%d calls=%d err=%v", n, len(f.reqs), err)
	}

	for _, reason := range []string{"memory_state", "org_policy", "zdr", "owner_required"} {
		f := &fakeUpserter{skipReasons: []string{"", reason}}
		n, err := NewSessionCoreSink(f, "org1", "user-1", "run-1", "thread-1").Persist(context.Background(), candidates)
		if err != nil || n != 1 || len(f.reqs) != 2 {
			t.Fatalf("%s must end the batch after the refusal: n=%d calls=%d err=%v", reason, n, len(f.reqs), err)
		}
	}
}

func TestPersist_SkippedProtectedNotCounted(t *testing.T) {
	f := &fakeUpserter{skipped: true}
	sink := NewSessionCoreSink(f, "org1", "user-1", "run-1", "thread-1")
	n, err := sink.Persist(context.Background(), []learning.SkillCandidate{
		{Name: "x", Content: "c"},
	})
	if err != nil || n != 0 {
		t.Fatalf("protected skip must persist 0 (no error), got n=%d err=%v", n, err)
	}
}

func TestPersist_RpcErrorPropagates(t *testing.T) {
	f := &fakeUpserter{err: errors.New("rpc down")}
	sink := NewSessionCoreSink(f, "org1", "user-1", "run-1", "thread-1")
	if _, err := sink.Persist(context.Background(), []learning.SkillCandidate{{Name: "x", Content: "c"}}); err == nil {
		t.Fatal("rpc error must propagate")
	}
}

// A candidate the content policy refuses is counted with its closed reason. It is
// neither an error, which would have the delivery retried, nor a reason to stop:
// the policy judges that candidate's text, not the owner, so the rest of the
// batch is still written.
func TestPersist_CountsPolicySkip(t *testing.T) {
	var refused []string
	previous := policyRefused
	policyRefused = func(_ context.Context, reason string) { refused = append(refused, reason) }
	t.Cleanup(func() { policyRefused = previous })

	candidates := []learning.SkillCandidate{{Name: "a", Content: "c"}, {Name: "b", Content: "c"}, {Name: "c", Content: "c"}}
	f := &fakeUpserter{skipReasons: []string{"skill_policy:injection_marker", "", ""}}
	n, err := NewSessionCoreSink(f, "org1", "user-1", "run-1", "thread-1").Persist(context.Background(), candidates)
	if err != nil || n != 2 || len(f.reqs) != 3 {
		t.Fatalf("a policy refusal must be neither an error nor the end of the batch: n=%d calls=%d err=%v", n, len(f.reqs), err)
	}
	if len(refused) != 1 || refused[0] != "skill_policy:injection_marker" {
		t.Fatalf("the refusal must be counted with its reason, got %v", refused)
	}
}

// Compile-time assertion that SessionCoreSink satisfies learning.Sink.
var _ learning.Sink = (*SessionCoreSink)(nil)
