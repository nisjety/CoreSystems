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
