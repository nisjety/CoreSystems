package feedback

import (
	"context"
	"errors"
	"testing"
)

func rating(orgID, runID, userID, skillID, value string) Rating {
	return Rating{
		OrgID:     orgID,
		UserID:    userID,
		RunID:     runID,
		SkillID:   skillID,
		FromScope: "agent",
		ToScope:   "workspace",
		Rating:    value,
	}
}

// A thumbs-UP must raise the promotion score, never lower it.
//
// This pins the subtlest of the four feedback-wire breaks. Before the fix,
// model-gateway forwarded the chat vocabulary ("positive") verbatim and this
// package counted only `rating == "good"`: the sample TOTAL went up while the
// GOOD count did not, so a thumbs-up *reduced* the skill's good-ratio. The two
// halves of the fix are (a) model-gateway normalises to the canonical
// vocabulary, and (b) a non-canonical rating is rejected here rather than
// counted, so it can never move the denominator alone.
func TestThumbsUpRaisesTheScoreAndNeverLowersIt(t *testing.T) {
	ctx := context.Background()
	s := NewMemoryStore()

	// Baseline: one good + one poor rating → 0.5.
	if err := s.Record(ctx, rating("org-1", "run-1", "user-1", "skill.x", RatingGood)); err != nil {
		t.Fatalf("record good: %v", err)
	}
	if err := s.Record(ctx, rating("org-1", "run-2", "user-2", "skill.x", RatingPoor)); err != nil {
		t.Fatalf("record poor: %v", err)
	}
	before := scoreOf(t, s, "skill.x")
	if before != 0.5 {
		t.Fatalf("baseline score = %v, want 0.5", before)
	}

	// A thumbs-up (already normalised to "good" by model-gateway) must RAISE it.
	if err := s.Record(ctx, rating("org-1", "run-3", "user-3", "skill.x", RatingGood)); err != nil {
		t.Fatalf("record thumbs-up: %v", err)
	}
	after := scoreOf(t, s, "skill.x")
	if after <= before {
		t.Fatalf("a thumbs-up lowered the score: %v → %v", before, after)
	}

	// The pre-fix wire value must be refused outright — accepting it is what
	// made a thumbs-up penalise the skill.
	err := s.Record(ctx, rating("org-1", "run-4", "user-4", "skill.x", "positive"))
	if !errors.Is(err, ErrUnknownRating) {
		t.Fatalf("raw chat vocabulary must be rejected, got %v", err)
	}
	if unchanged := scoreOf(t, s, "skill.x"); unchanged != after {
		t.Fatalf("a rejected rating moved the score: %v → %v", after, unchanged)
	}
}

func scoreOf(t *testing.T, s Store, skillID string) float64 {
	t.Helper()
	got, err := s.Candidates(context.Background(), 1, 0.0)
	if err != nil {
		t.Fatalf("candidates: %v", err)
	}
	for _, c := range got {
		if c.SkillID == skillID {
			return c.Score
		}
	}
	t.Fatalf("skill %q not found in %+v", skillID, got)
	return 0
}

func TestCandidatesHonourThresholdAndSampleFloor(t *testing.T) {
	ctx := context.Background()
	s := NewMemoryStore()

	// 9/10 good, enough samples → candidate.
	for i := 0; i < 9; i++ {
		mustRecord(t, s, rating("org-1", "run-good-"+itoa(i), "user-"+itoa(i), "skill.good", RatingGood))
	}
	mustRecord(t, s, rating("org-1", "run-good-9", "user-9", "skill.good", RatingPoor))

	// 5/10 → below threshold.
	for i := 0; i < 5; i++ {
		mustRecord(t, s, rating("org-1", "run-meh-"+itoa(i), "user-"+itoa(i), "skill.meh", RatingGood))
	}
	for i := 5; i < 10; i++ {
		mustRecord(t, s, rating("org-1", "run-meh-"+itoa(i), "user-"+itoa(i), "skill.meh", RatingPoor))
	}

	// 1/1 → too few samples.
	mustRecord(t, s, rating("org-1", "run-new", "user-0", "skill.new", RatingGood))

	got, err := s.Candidates(ctx, 5, 0.8)
	if err != nil {
		t.Fatalf("candidates: %v", err)
	}
	if len(got) != 1 {
		t.Fatalf("expected 1 candidate, got %d: %+v", len(got), got)
	}
	if got[0].SkillID != "skill.good" || got[0].Good != 9 || got[0].Total != 10 {
		t.Fatalf("unexpected candidate: %+v", got[0])
	}
	if got[0].OrgID != "org-1" {
		t.Fatalf("candidate lost its tenant: %+v", got[0])
	}
}

// A run-level rating is recorded but is not a promotion candidate: a chat turn
// with no injected skill has no skill to promote. Before the fix this rating was
// dropped entirely (`if skillID == "" { return }`).
func TestRunOnlyRatingIsRecordedButIsNotAPromotionCandidate(t *testing.T) {
	ctx := context.Background()
	s := NewMemoryStore()
	mustRecord(t, s, rating("org-1", "run-1", "user-1", "", RatingPoor))

	got, err := s.Candidates(ctx, 1, 0.0)
	if err != nil {
		t.Fatalf("candidates: %v", err)
	}
	if len(got) != 0 {
		t.Fatalf("a run-only rating must not be a promotion candidate: %+v", got)
	}
	// But it was accepted, not dropped on the floor.
	if err := s.Record(ctx, rating("org-1", "run-2", "user-1", "", RatingGood)); err != nil {
		t.Fatalf("run-only rating must be accepted: %v", err)
	}
}

// One rater changing their mind must replace their sample, not add another.
func TestReRatingReplacesRatherThanAccumulates(t *testing.T) {
	ctx := context.Background()
	s := NewMemoryStore()
	mustRecord(t, s, rating("org-1", "run-1", "user-1", "skill.x", RatingGood))
	mustRecord(t, s, rating("org-1", "run-1", "user-1", "skill.x", RatingPoor))

	got, err := s.Candidates(ctx, 1, 0.0)
	if err != nil {
		t.Fatalf("candidates: %v", err)
	}
	if len(got) != 1 {
		t.Fatalf("expected 1 candidate, got %+v", got)
	}
	if got[0].Total != 1 || got[0].Good != 0 {
		t.Fatalf("re-rating accumulated instead of replacing: %+v", got[0])
	}
}

// Two tenants rating the same skill id must not be pooled into one score.
func TestRatingsAreNeverPooledAcrossTenants(t *testing.T) {
	ctx := context.Background()
	s := NewMemoryStore()
	mustRecord(t, s, rating("org-a", "run-1", "user-1", "skill.shared", RatingGood))
	mustRecord(t, s, rating("org-b", "run-2", "user-2", "skill.shared", RatingPoor))

	got, err := s.Candidates(ctx, 1, 0.0)
	if err != nil {
		t.Fatalf("candidates: %v", err)
	}
	if len(got) != 2 {
		t.Fatalf("expected one candidate per tenant, got %+v", got)
	}
	for _, c := range got {
		if c.Total != 1 {
			t.Fatalf("tenant %s aggregated another tenant's rating: %+v", c.OrgID, c)
		}
	}
}

func TestNormalizeRejectsIncompleteRatings(t *testing.T) {
	cases := map[string]Rating{
		"missing org":     {RunID: "run-1", Rating: RatingGood},
		"missing run":     {OrgID: "org-1", Rating: RatingGood},
		"missing rating":  {OrgID: "org-1", RunID: "run-1"},
		"unknown rating":  {OrgID: "org-1", RunID: "run-1", Rating: "positive"},
		"unknown rating2": {OrgID: "org-1", RunID: "run-1", Rating: "negative"},
	}
	for name, r := range cases {
		t.Run(name, func(t *testing.T) {
			if _, err := Normalize(r); err == nil {
				t.Fatalf("expected an error for %+v", r)
			}
		})
	}
}

func TestNormalizeAcceptsEveryCanonicalRating(t *testing.T) {
	for _, value := range []string{RatingGood, RatingAcceptable, RatingPoor, " GOOD "} {
		if _, err := Normalize(Rating{OrgID: "o", RunID: "r", Rating: value}); err != nil {
			t.Fatalf("canonical rating %q rejected: %v", value, err)
		}
	}
}

func TestMemoryStoreDeclaresItselfNonDurable(t *testing.T) {
	if NewMemoryStore().Durable() {
		t.Fatal("MemoryStore must not claim durability")
	}
}

func mustRecord(t *testing.T, s Store, r Rating) {
	t.Helper()
	if err := s.Record(context.Background(), r); err != nil {
		t.Fatalf("record %+v: %v", r, err)
	}
}

func itoa(i int) string {
	if i == 0 {
		return "0"
	}
	var digits []byte
	for i > 0 {
		digits = append([]byte{byte('0' + i%10)}, digits...)
		i /= 10
	}
	return string(digits)
}
