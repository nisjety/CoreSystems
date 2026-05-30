package learning

import "testing"

func cand(name, content string, conf float64) SkillCandidate {
	return SkillCandidate{
		Name:       name,
		Content:    content,
		Confidence: conf,
	}
}

func TestSelectForPersistence_DropsLowConfidenceAndInvalid(t *testing.T) {
	in := []SkillCandidate{
		cand("good", "do the thing carefully", 0.9),
		cand("weak", "maybe useful", 0.2),  // below MinConfidence
		cand("", "no name", 0.9),           // invalid
		cand("empty-content", "", 0.9),     // invalid
	}
	out := SelectForPersistence(in, nil)
	if len(out) != 1 || out[0].Name != "good" {
		t.Fatalf("expected only 'good' to survive, got %+v", out)
	}
	if out[0].Origin != OriginBackgroundReview {
		t.Fatalf("survivor must be tagged background_review, got %q", out[0].Origin)
	}
}

func TestSelectForPersistence_ProtectsUserAuthoredSkills(t *testing.T) {
	// A review must never overwrite a human-authored skill with the same name.
	existing := []ExistingSkill{{Name: "Deploy Runbook", Origin: OriginUser}}
	in := []SkillCandidate{
		cand("deploy runbook", "machine version", 0.95), // name collides (case-insensitive)
		cand("New Skill", "genuinely new", 0.95),
	}
	out := SelectForPersistence(in, existing)
	if len(out) != 1 || out[0].Name != "New Skill" {
		t.Fatalf("user skill must be protected; expected only 'New Skill', got %+v", out)
	}
}

func TestSelectForPersistence_DedupsAgainstExistingHash(t *testing.T) {
	c := cand("Cache Tips", "use the shared cache", 0.9)
	existing := []ExistingSkill{{Name: "Cache Tips", ContentHash: c.ContentHash(), Origin: OriginBackgroundReview}}
	out := SelectForPersistence([]SkillCandidate{c}, existing)
	if len(out) != 0 {
		t.Fatalf("identical existing skill must dedup to nothing, got %+v", out)
	}
}

func TestSelectForPersistence_DedupsWithinBatch(t *testing.T) {
	in := []SkillCandidate{
		cand("Tips", "same body", 0.9),
		cand("Tips", "same body", 0.9), // duplicate within batch
	}
	out := SelectForPersistence(in, nil)
	if len(out) != 1 {
		t.Fatalf("within-batch duplicates must collapse to 1, got %d", len(out))
	}
}

func TestContentHash_StableAndNameNormalized(t *testing.T) {
	a := cand("  Cache Tips ", "body", 0.9)
	b := cand("cache tips", "body", 0.9)
	if a.ContentHash() != b.ContentHash() {
		t.Fatal("hash must be insensitive to name case/whitespace")
	}
	diff := cand("cache tips", "different body", 0.9)
	if a.ContentHash() == diff.ContentHash() {
		t.Fatal("different content must produce a different hash")
	}
}

func TestSelectForPersistence_PreservesOrderAndForcesOrigin(t *testing.T) {
	in := []SkillCandidate{
		{Name: "first", Content: "a", Confidence: 0.8, Origin: OriginUser}, // spoofed origin
		{Name: "second", Content: "b", Confidence: 0.8},
	}
	out := SelectForPersistence(in, nil)
	if len(out) != 2 || out[0].Name != "first" || out[1].Name != "second" {
		t.Fatalf("order must be preserved, got %+v", out)
	}
	for _, c := range out {
		if c.Origin != OriginBackgroundReview {
			t.Fatalf("caller-supplied origin must be overridden; got %q for %q", c.Origin, c.Name)
		}
	}
}
