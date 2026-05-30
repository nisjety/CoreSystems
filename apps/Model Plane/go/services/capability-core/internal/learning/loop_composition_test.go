package learning_test

// Integration test for the ASSEMBLED G7 learning loop (matrix §G7). The unit
// tests cover llmreviewer, skillsink, and RunReview/SelectForPersistence in
// isolation; this wires the REAL llmreviewer + REAL skillsink through
// learning.RunReview, faking ONLY the two external boundaries (the LLM call and
// the session-core RPC). It verifies the composition — reviewer.Review ->
// SelectForPersistence (provenance + dedup + threshold) -> sink.Persist — holds
// end to end, which the per-part unit tests cannot prove on their own.
//
// External test package (learning_test) so it can import llmreviewer/skillsink
// (which import learning) without an import cycle.

import (
	"context"
	"testing"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/capability-core/internal/learning"
	"github.com/triodelab/model-plane/services/capability-core/internal/llmreviewer"
	"github.com/triodelab/model-plane/services/capability-core/internal/skillsink"
	"google.golang.org/grpc"
)

// fakeInferer is the untrusted LLM boundary: returns a canned model reply.
type fakeInferer struct {
	content string
	calls   int
}

func (f *fakeInferer) Infer(
	_ context.Context,
	_ *mpv1.InferRequest,
	_ ...grpc.CallOption,
) (*mpv1.InferResponse, error) {
	f.calls++
	return &mpv1.InferResponse{Content: f.content}, nil
}

// fakeUpserter is the session-core boundary: records writes and can report
// skipped_protected for a given name to simulate the server-side provenance
// guard rejecting an overwrite of a human-authored skill.
type fakeUpserter struct {
	got       []*mpv1.UpsertAgentSkillRequest
	protected map[string]bool
}

func (f *fakeUpserter) UpsertAgentSkill(
	_ context.Context,
	in *mpv1.UpsertAgentSkillRequest,
	_ ...grpc.CallOption,
) (*mpv1.UpsertAgentSkillResponse, error) {
	f.got = append(f.got, in)
	return &mpv1.UpsertAgentSkillResponse{SkippedProtected: f.protected[in.GetName()]}, nil
}

func TestG7Loop_FiltersAndPersistsAcrossTheWholeChain(t *testing.T) {
	// Model proposes three: one good & new, one duplicating an existing USER
	// skill (must be dropped by provenance), one below the confidence floor
	// (must be dropped).
	reply := `{"skills":[
		{"name":"Cache Tips","description":"caching guidance","content":"use the cache","trigger_keywords":["cache"],"confidence":0.9},
		{"name":"Runbook","description":"d","content":"machine version","confidence":0.95},
		{"name":"Flaky Idea","description":"d","content":"meh","confidence":0.2}
	]}`
	inferer := &fakeInferer{content: reply}
	upserter := &fakeUpserter{protected: map[string]bool{}}

	reviewer := llmreviewer.NewReviewer(inferer, "test-model", "org-42")
	sink := skillsink.NewSessionCoreSink(upserter, "org-42")
	existing := []learning.ExistingSkill{
		{Name: "Runbook", Origin: learning.OriginUser}, // human-authored — protected
	}

	n, err := learning.RunReview(context.Background(), "a session transcript", existing, reviewer, sink)
	if err != nil {
		t.Fatalf("RunReview: %v", err)
	}
	if inferer.calls != 1 {
		t.Fatalf("reviewer should have called the model once, got %d", inferer.calls)
	}
	// Only "Cache Tips" survives: "Runbook" dropped by provenance, "Flaky Idea"
	// dropped by the confidence floor.
	if len(upserter.got) != 1 {
		t.Fatalf("expected exactly 1 skill to reach the sink, got %d: %+v", len(upserter.got), upserter.got)
	}
	w := upserter.got[0]
	if w.GetName() != "Cache Tips" {
		t.Fatalf("wrong skill persisted: %q", w.GetName())
	}
	if w.GetOrigin() != string(learning.OriginBackgroundReview) {
		t.Fatalf("origin must be background_review end-to-end, got %q", w.GetOrigin())
	}
	if w.GetOrgId() != "org-42" {
		t.Fatalf("org scoping lost across the chain: %q", w.GetOrgId())
	}
	if n != 1 {
		t.Fatalf("expected 1 persisted, got %d", n)
	}
}

func TestG7Loop_SinkProvenanceSkipIsNotCountedNorError(t *testing.T) {
	reply := `{"skills":[{"name":"Cache Tips","content":"use cache","confidence":0.9}]}`
	inferer := &fakeInferer{content: reply}
	// Server-side guard rejects the write (target is a protected user skill).
	upserter := &fakeUpserter{protected: map[string]bool{"Cache Tips": true}}

	reviewer := llmreviewer.NewReviewer(inferer, "m", "org-1")
	sink := skillsink.NewSessionCoreSink(upserter, "org-1")

	n, err := learning.RunReview(context.Background(), "transcript", nil, reviewer, sink)
	if err != nil {
		t.Fatalf("RunReview: %v", err)
	}
	if len(upserter.got) != 1 {
		t.Fatalf("expected 1 upsert attempt, got %d", len(upserter.got))
	}
	if n != 0 {
		t.Fatalf("skipped_protected must not be counted as persisted, got n=%d", n)
	}
}

func TestG7Loop_NothingRetainablePersistsZero(t *testing.T) {
	// Everything below the confidence floor -> nothing reaches the sink.
	reply := `{"skills":[{"name":"Weak","content":"x","confidence":0.1}]}`
	inferer := &fakeInferer{content: reply}
	upserter := &fakeUpserter{}

	reviewer := llmreviewer.NewReviewer(inferer, "m", "org-1")
	sink := skillsink.NewSessionCoreSink(upserter, "org-1")

	n, err := learning.RunReview(context.Background(), "transcript", nil, reviewer, sink)
	if err != nil {
		t.Fatalf("RunReview: %v", err)
	}
	if n != 0 || len(upserter.got) != 0 {
		t.Fatalf("nothing retainable should persist 0 and not call the sink; got n=%d calls=%d", n, len(upserter.got))
	}
}
