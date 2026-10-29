package sessionreview

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"

	"github.com/triodelab/model-plane/pkg/envelope"
	"github.com/triodelab/model-plane/services/capability-core/internal/learning"
)

func env(eventType, resourceRef, org, payload string) *envelope.Envelope {
	return &envelope.Envelope{
		EventType:   eventType,
		OrgID:       org,
		ResourceRef: resourceRef,
		Payload:     json.RawMessage(payload),
	}
}

func TestParseRunCompleted_ValidWithThreadId(t *testing.T) {
	ref, ok := ParseRunCompleted(env(RunCompletedEventType, "run/r-1", "org-9", `{"thread_id":"t-1"}`))
	if !ok {
		t.Fatal("expected ok")
	}
	if ref.OrgID != "org-9" || ref.RunID != "r-1" || ref.ThreadID != "t-1" {
		t.Fatalf("bad ref: %+v", ref)
	}
}

func TestParseRunCompleted_ToleratesColonAndMissingPayload(t *testing.T) {
	ref, ok := ParseRunCompleted(env(RunCompletedEventType, "run:r-2", "o", ""))
	if !ok || ref.RunID != "r-2" || ref.ThreadID != "" {
		t.Fatalf("colon form / empty payload mishandled: %+v ok=%v", ref, ok)
	}
}

func TestParseRunCompleted_RejectsNonTriggers(t *testing.T) {
	cases := []struct {
		name                        string
		eventType, resourceRef, org string
	}{
		{"wrong event type", "STEP_COMPLETED", "run/r-1", "o"},
		{"empty org", RunCompletedEventType, "run/r-1", ""},
		{"no run in ref", RunCompletedEventType, "thread/t-1", "o"},
		{"empty run id", RunCompletedEventType, "run/", "o"},
	}
	for _, c := range cases {
		if _, ok := ParseRunCompleted(env(c.eventType, c.resourceRef, c.org, "")); ok {
			t.Fatalf("%s: expected reject", c.name)
		}
	}
	if _, ok := ParseRunCompleted(nil); ok {
		t.Fatal("nil envelope must reject")
	}
}

// --- OnRunCompleted orchestration (fakes at every boundary) ---

type fakeSrc struct {
	transcript string
	existing   []learning.ExistingSkill
	err        error
	gotRef     SessionRef
}

func (f *fakeSrc) Fetch(_ context.Context, ref SessionRef) (string, []learning.ExistingSkill, error) {
	f.gotRef = ref
	return f.transcript, f.existing, f.err
}

type fakeReviewer struct {
	out []learning.SkillCandidate
	err error
}

func (f *fakeReviewer) Review(_ context.Context, _ string, _ []learning.ExistingSkill, _ string) ([]learning.SkillCandidate, error) {
	return f.out, f.err
}

type fakeSink struct{ got []learning.SkillCandidate }

func (f *fakeSink) Persist(_ context.Context, skills []learning.SkillCandidate) (int, error) {
	f.got = append(f.got, skills...)
	return len(skills), nil
}

func TestOnRunCompleted_FetchesThenReviewsAndPersists(t *testing.T) {
	ref := SessionRef{OrgID: "org-9", RunID: "r-1", ThreadID: "t-1"}
	src := &fakeSrc{transcript: "did useful things", existing: nil}
	// A valid, above-threshold candidate survives SelectForPersistence.
	reviewer := &fakeReviewer{out: []learning.SkillCandidate{
		{Name: "Cache", Content: "use the cache", Confidence: 0.9, Origin: learning.OriginBackgroundReview},
	}}
	sink := &fakeSink{}

	n, err := OnRunCompleted(context.Background(), ref, src, reviewer, sink)
	if err != nil {
		t.Fatalf("OnRunCompleted: %v", err)
	}
	if n != 1 {
		t.Fatalf("expected 1 persisted, got %d", n)
	}
	if src.gotRef != ref {
		t.Fatalf("ref not forwarded to source: %+v", src.gotRef)
	}
	if len(sink.got) != 1 || sink.got[0].Name != "Cache" {
		t.Fatalf("sink did not receive the surviving candidate: %+v", sink.got)
	}
}

func TestOnRunCompleted_FetchErrorIsWrapped(t *testing.T) {
	src := &fakeSrc{err: errors.New("session-core down")}
	_, err := OnRunCompleted(context.Background(), SessionRef{RunID: "r-1"}, src,
		&fakeReviewer{}, &fakeSink{})
	if err == nil || !strings.Contains(err.Error(), "fetch transcript") {
		t.Fatalf("expected wrapped fetch error, got %v", err)
	}
}

func TestOnRunCompleted_NilDepsError(t *testing.T) {
	if _, err := OnRunCompleted(context.Background(), SessionRef{}, nil, &fakeReviewer{}, &fakeSink{}); err == nil {
		t.Fatal("nil source must error")
	}
}
