package activities

import (
	"context"
	"strconv"
	"testing"

	"github.com/triodelab/model-plane/services/orchestrator-core/internal/feedback"
)

// The rating counters themselves are tested in internal/feedback (including the
// thumbs-up-must-not-lower-the-score inversion). What matters here is that the
// activity reads the durable store and preserves the tenant on the way to the
// promotion workflow.
func TestAggregateFeedbackActivity_ReadsDurableStore(t *testing.T) {
	ctx := context.Background()
	store := feedback.NewMemoryStore()
	// 30 good + 1 poor. The raw ratio (0.97) is not what promotes it — the
	// Wilson lower bound is, and that needs VOLUME as well as agreement. The
	// four-of-five sample this test used to carry has a bound of ~0.38 and no
	// longer clears 0.8, which is the intended behaviour change rather than a
	// regression: ten-ish samples is thin evidence however good the ratio looks.
	users := make([]string, 0, 31)
	for i := range 31 {
		users = append(users, "u"+strconv.Itoa(i))
	}
	for i, user := range users {
		r := feedback.Rating{
			OrgID:     "org-1",
			UserID:    user,
			RunID:     "run-" + user,
			SkillID:   "cap.skill.summarize",
			FromScope: "agent",
			ToScope:   "workspace",
			Rating:    feedback.RatingGood,
		}
		if i == 30 {
			r.Rating = feedback.RatingPoor
		}
		if err := store.Record(ctx, r); err != nil {
			t.Fatalf("record: %v", err)
		}
	}

	a := NewActivities(nil, nil)
	a.SetFeedbackStore(store)

	out, err := a.AggregateFeedbackActivity(ctx, FeedbackAggregateInput{MinSamples: 5, PromoteThreshold: 0.8})
	if err != nil {
		t.Fatalf("aggregate: %v", err)
	}
	if len(out.Candidates) != 1 {
		t.Fatalf("expected 1 candidate, got %+v", out.Candidates)
	}
	got := out.Candidates[0]
	if got.SkillID != "cap.skill.summarize" || got.Good != 30 || got.Total != 31 {
		t.Fatalf("unexpected candidate: %+v", got)
	}
	if got.OrgID != "org-1" {
		t.Fatalf("candidate lost its tenant: %+v", got)
	}
}

// No store configured must be an empty sweep, never a panic.
func TestAggregateFeedbackActivity_WithoutStoreIsEmpty(t *testing.T) {
	a := NewActivities(nil, nil)
	out, err := a.AggregateFeedbackActivity(context.Background(), FeedbackAggregateInput{})
	if err != nil {
		t.Fatalf("aggregate: %v", err)
	}
	if len(out.Candidates) != 0 {
		t.Fatalf("expected no candidates, got %+v", out.Candidates)
	}
}

func TestRecordFeedback_WithoutStoreIsAnError(t *testing.T) {
	a := NewActivities(nil, nil)
	if err := a.RecordFeedback(context.Background(), feedback.Rating{
		OrgID: "org-1", RunID: "run-1", Rating: feedback.RatingGood,
	}); err == nil {
		t.Fatal("a missing store must surface, not silently drop the rating")
	}
}

// TestExecuteStepRequest_ThreadsViewerScope verifies the ExecuteStep RPC request
// carries the tenant (org_id) AND acting viewer (user_id), closing the proto gap
// that made the primitive step path org-scoped only (Phase 4).
func TestExecuteStepRequest_ThreadsViewerScope(t *testing.T) {
	cases := []struct {
		name  string
		input StepLoopInput
	}{
		{"full scope", StepLoopInput{RunID: "run-1", OrgID: "org-1", UserID: "user-1"}},
		{"org only (legacy)", StepLoopInput{RunID: "run-2", OrgID: "org-2"}},
		{"empty", StepLoopInput{}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			req := executeStepRequest(tc.input)
			if req.RunId != tc.input.RunID {
				t.Errorf("RunId = %q, want %q", req.RunId, tc.input.RunID)
			}
			if req.OrgId != tc.input.OrgID {
				t.Errorf("OrgId = %q, want %q", req.OrgId, tc.input.OrgID)
			}
			if req.UserId != tc.input.UserID {
				t.Errorf("UserId = %q, want %q (viewer scope must be threaded)", req.UserId, tc.input.UserID)
			}
		})
	}
}
