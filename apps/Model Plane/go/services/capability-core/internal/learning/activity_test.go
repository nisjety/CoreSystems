package learning

import (
	"context"
	"errors"
	"testing"
)

type fakeReviewer struct {
	out []SkillCandidate
	err error
	// captured
	gotPrompt string
}

func (f *fakeReviewer) Review(_ context.Context, _ string, _ []ExistingSkill, prompt string) ([]SkillCandidate, error) {
	f.gotPrompt = prompt
	return f.out, f.err
}

type fakeSink struct {
	got []SkillCandidate
	err error
}

func (f *fakeSink) Persist(_ context.Context, skills []SkillCandidate) (int, error) {
	if f.err != nil {
		return 0, f.err
	}
	f.got = append(f.got, skills...)
	return len(skills), nil
}

func TestRunReview_EmptyTranscriptIsNoop(t *testing.T) {
	r := &fakeReviewer{out: []SkillCandidate{cand("x", "y", 0.9)}}
	s := &fakeSink{}
	n, err := RunReview(context.Background(), "   ", nil, r, s)
	if err != nil || n != 0 {
		t.Fatalf("empty transcript must be a no-op, got n=%d err=%v", n, err)
	}
	if len(s.got) != 0 {
		t.Fatalf("nothing should be persisted, got %d", len(s.got))
	}
}

func TestRunReview_PersistsOnlyPolicySurvivors(t *testing.T) {
	// One strong candidate, one below threshold, one colliding with a
	// protected user skill — only the strong fresh one should persist.
	existing := []ExistingSkill{{Name: "Protected", Origin: OriginUser}}
	r := &fakeReviewer{out: []SkillCandidate{
		cand("Good Skill", "do it well", 0.9),
		cand("weak", "meh", 0.1),
		cand("protected", "machine overwrite attempt", 0.95),
	}}
	s := &fakeSink{}
	n, err := RunReview(context.Background(), "session transcript", existing, r, s)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if n != 1 || len(s.got) != 1 || s.got[0].Name != "Good Skill" {
		t.Fatalf("expected only 'Good Skill' persisted, got n=%d %+v", n, s.got)
	}
	if s.got[0].Origin != OriginBackgroundReview {
		t.Fatalf("persisted skill must be tagged background_review, got %q", s.got[0].Origin)
	}
	if r.gotPrompt != ReviewPrompt {
		t.Fatal("reviewer must be called with the canonical ReviewPrompt")
	}
}

func TestRunReview_ReviewerErrorPropagates(t *testing.T) {
	r := &fakeReviewer{err: errors.New("model unavailable")}
	_, err := RunReview(context.Background(), "t", nil, r, &fakeSink{})
	if err == nil {
		t.Fatal("reviewer error must propagate")
	}
}

func TestRunReview_NothingRetainablePersistsZero(t *testing.T) {
	r := &fakeReviewer{out: []SkillCandidate{cand("weak", "meh", 0.1)}}
	s := &fakeSink{}
	n, err := RunReview(context.Background(), "t", nil, r, s)
	if err != nil || n != 0 {
		t.Fatalf("sub-threshold review persists zero, got n=%d err=%v", n, err)
	}
}

func TestRunReview_SinkErrorPropagates(t *testing.T) {
	r := &fakeReviewer{out: []SkillCandidate{cand("Good", "solid content", 0.9)}}
	s := &fakeSink{err: errors.New("db down")}
	_, err := RunReview(context.Background(), "t", nil, r, s)
	if err == nil {
		t.Fatal("sink error must propagate")
	}
}

func TestRunReview_RequiresDeps(t *testing.T) {
	if _, err := RunReview(context.Background(), "t", nil, nil, &fakeSink{}); err == nil {
		t.Fatal("nil reviewer must error")
	}
}
