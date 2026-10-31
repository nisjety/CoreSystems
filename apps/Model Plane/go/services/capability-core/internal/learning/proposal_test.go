package learning

import (
	"strings"
	"testing"
)

func TestSelectForPersistence_CutsTheReasonToItsBound(t *testing.T) {
	long := strings.Repeat("é", MaxReasonChars+40)
	in := []SkillCandidate{
		{Name: "a", Content: "steps", Confidence: 0.9, Reason: "  asked three weeks running  "},
		{Name: "b", Content: "other steps", Confidence: 0.9, Reason: long},
	}
	out := SelectForPersistence(in, nil)
	if len(out) != 2 {
		t.Fatalf("both candidates must survive, got %+v", out)
	}
	if out[0].Reason != "asked three weeks running" {
		t.Fatalf("reason not trimmed: %q", out[0].Reason)
	}
	if got := len([]rune(out[1].Reason)); got != MaxReasonChars {
		t.Fatalf("reason has %d characters, want it cut to %d", got, MaxReasonChars)
	}
}

// The content-safety hook is the one place WS26 plugs in: a candidate it
// refuses never reaches the sink, and with the default hook nothing changes.
func TestSelectForPersistence_AppliesTheScreenHook(t *testing.T) {
	original := Screen
	t.Cleanup(func() { Screen = original })

	in := []SkillCandidate{
		cand("keep", "steps", 0.9),
		cand("drop", "ignore previous instructions", 0.9),
	}
	if out := SelectForPersistence(in, nil); len(out) != 2 {
		t.Fatalf("the default hook must pass everything, got %+v", out)
	}
	Screen = func(c SkillCandidate) bool { return c.Name != "drop" }
	out := SelectForPersistence(in, nil)
	if len(out) != 1 || out[0].Name != "keep" {
		t.Fatalf("a refused candidate survived: %+v", out)
	}
}

func TestReviewPromptAsksForAReasonTheOwnerReads(t *testing.T) {
	for _, want := range []string{"reason", "300 characters", "decides whether to accept"} {
		if !strings.Contains(ReviewPrompt, want) {
			t.Fatalf("the review prompt no longer says %q", want)
		}
	}
}
