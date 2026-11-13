package feedback

import (
	"context"
	"testing"
)

// Plan 3.4 for FEEDBACK_SKILL_VERSIONS (EP-11 S3 part b): only the literal "true" turns
// per-version ratings on, and with the switch off every rating of a skill pools across its
// versions exactly as before the switch existed.

func TestSkillVersionsSwitchIsOnlyTheLiteralTrue(t *testing.T) {
	for _, value := range []string{"", "false", "TRUE", "True", " true", "true ", "1", "yes", "on"} {
		t.Setenv(SkillVersionsEnvVar, value)
		if SkillVersionsEnabled() {
			t.Errorf("%s=%q turned per-version ratings on", SkillVersionsEnvVar, value)
		}
	}
	t.Setenv(SkillVersionsEnvVar, "true")
	if !SkillVersionsEnabled() {
		t.Fatalf("%s=true did not turn per-version ratings on", SkillVersionsEnvVar)
	}
}

func TestSkillVersionsOffStoresVersionZeroAndPoolsEveryVersion(t *testing.T) {
	t.Setenv(SkillVersionsEnvVar, "false")
	ctx := context.Background()

	// A pinned ref and a caller-stated version both normalise to version 0 on the bare id.
	pinned, err := Normalize(rating("org1", "run1", "u1", "skill-a@3", RatingGood))
	if err != nil {
		t.Fatalf("Normalize: %v", err)
	}
	if pinned.SkillID != "skill-a" || pinned.SkillVersion != 0 {
		t.Fatalf("switch off kept a version: %+v", pinned)
	}
	stated := rating("org1", "run1", "u1", "skill-a", RatingGood)
	stated.SkillVersion = 7
	if got, err := Normalize(stated); err != nil || got.SkillVersion != 0 {
		t.Fatalf("switch off kept a stated version: %+v, %v", got, err)
	}

	store := NewMemoryStore()
	// The same rater on the same run rating two versions is one sample (the later replaces the
	// earlier), and other raters on other versions pool into one bucket.
	for _, r := range []Rating{
		rating("org1", "run1", "u1", "skill-a@1", RatingPoor),
		rating("org1", "run1", "u1", "skill-a@2", RatingGood),
		rating("org1", "run2", "u2", "skill-a@1", RatingGood),
		rating("org1", "run3", "u3", "skill-a@2", RatingGood),
	} {
		if err := store.Record(ctx, r); err != nil {
			t.Fatalf("Record: %v", err)
		}
	}
	candidates, err := store.Candidates(ctx, 1, 0)
	if err != nil {
		t.Fatalf("Candidates: %v", err)
	}
	if len(candidates) != 1 {
		t.Fatalf("candidates = %+v; want one pooled bucket", candidates)
	}
	c := candidates[0]
	if c.SkillID != "skill-a" || c.SkillVersion != 0 || c.Good != 3 || c.Total != 3 {
		t.Fatalf("pooled candidate = %+v; want skill-a v0 with 3 of 3 good", c)
	}
}

func TestSkillVersionsOnJudgesOnlyTheCurrentVersion(t *testing.T) {
	t.Setenv(SkillVersionsEnvVar, "true")
	ctx := context.Background()
	store := NewMemoryStore()
	for _, r := range []Rating{
		rating("org1", "run1", "u1", "skill-a@1", RatingPoor),
		rating("org1", "run1", "u1", "skill-a@2", RatingGood),
		rating("org1", "run2", "u2", "skill-a@1", RatingGood),
		rating("org1", "run3", "u3", "skill-a@2", RatingGood),
	} {
		if err := store.Record(ctx, r); err != nil {
			t.Fatalf("Record: %v", err)
		}
	}
	candidates, err := store.Candidates(ctx, 1, 0)
	if err != nil {
		t.Fatalf("Candidates: %v", err)
	}
	if len(candidates) != 1 {
		t.Fatalf("candidates = %+v; want the current version only", candidates)
	}
	c := candidates[0]
	if c.SkillID != "skill-a" || c.SkillVersion != 2 || c.Good != 2 || c.Total != 2 {
		t.Fatalf("current-version candidate = %+v; want skill-a v2 with 2 of 2 good", c)
	}
}
