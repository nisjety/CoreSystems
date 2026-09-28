package learning

import (
	"context"
	"errors"
	"testing"
)

func TestRunReviewOutcome_SkipsEmptyTranscriptWithoutReview(t *testing.T) {
	r := &fakeReviewer{out: []SkillCandidate{cand("x", "y", 0.9)}}
	got, err := RunReviewOutcome(context.Background(), " \n ", nil, r, &fakeSink{})
	if err != nil || got != (ReviewOutcome{Skipped: SkipEmptyTranscript}) {
		t.Fatalf("outcome = %+v err=%v, want empty_transcript skip", got, err)
	}
	if r.gotPrompt != "" {
		t.Fatal("an empty transcript must never reach the reviewer")
	}
}

func TestRunReviewOutcome_CountsEachStage(t *testing.T) {
	r := &fakeReviewer{out: []SkillCandidate{
		cand("Keep", "useful", 0.9),
		cand("Low", "weak", 0.2),
		cand("Keep", "useful", 0.9), // in-batch duplicate
	}}
	got, err := RunReviewOutcome(context.Background(), "t", nil, r, &fakeSink{})
	if err != nil {
		t.Fatal(err)
	}
	if want := (ReviewOutcome{Candidates: 3, Selected: 1, Persisted: 1}); got != want {
		t.Fatalf("outcome = %+v, want %+v", got, want)
	}
}

func TestRunReviewOutcome_ReviewedButNothingSelectedIsNotASkip(t *testing.T) {
	r := &fakeReviewer{out: []SkillCandidate{cand("Low", "weak", 0.1)}}
	got, err := RunReviewOutcome(context.Background(), "t", nil, r, &fakeSink{})
	if err != nil || got != (ReviewOutcome{Candidates: 1}) {
		t.Fatalf("outcome = %+v err=%v", got, err)
	}
}

func TestRunReviewOutcome_PersistFailureKeepsCountsButNotPersisted(t *testing.T) {
	r := &fakeReviewer{out: []SkillCandidate{cand("Keep", "useful", 0.9)}}
	got, err := RunReviewOutcome(context.Background(), "t", nil, r, &fakeSink{err: errors.New("down")})
	if err == nil || got != (ReviewOutcome{Candidates: 1, Selected: 1}) {
		t.Fatalf("outcome = %+v err=%v", got, err)
	}
}
