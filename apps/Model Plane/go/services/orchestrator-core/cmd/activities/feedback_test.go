package activities

import "testing"

func TestFeedbackStore_CandidatesThresholdAndSamples(t *testing.T) {
	s := NewFeedbackStore()

	// High score, enough samples → candidate.
	for i := 0; i < 9; i++ {
		s.Record("skill.good", "agent", "workspace", "good")
	}
	s.Record("skill.good", "agent", "workspace", "poor") // 9/10 = 0.9

	// Enough samples but below threshold → excluded.
	for i := 0; i < 5; i++ {
		s.Record("skill.meh", "agent", "workspace", "good")
	}
	for i := 0; i < 5; i++ {
		s.Record("skill.meh", "agent", "workspace", "poor") // 5/10 = 0.5
	}

	// High score but too few samples → excluded.
	s.Record("skill.new", "agent", "workspace", "good") // 1/1

	got := s.Candidates(5, 0.8)
	if len(got) != 1 {
		t.Fatalf("expected 1 candidate, got %d: %+v", len(got), got)
	}
	if got[0].SkillID != "skill.good" {
		t.Fatalf("expected skill.good, got %s", got[0].SkillID)
	}
	if got[0].Total != 10 || got[0].Good != 9 {
		t.Fatalf("unexpected counts: good=%d total=%d", got[0].Good, got[0].Total)
	}
}

func TestFeedbackStore_EmptyRatingAndSkillIgnored(t *testing.T) {
	s := NewFeedbackStore()
	s.Record("", "agent", "workspace", "good") // empty skill ignored
	if c := s.Candidates(1, 0.0); len(c) != 0 {
		t.Fatalf("empty skill should not be recorded, got %d", len(c))
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
