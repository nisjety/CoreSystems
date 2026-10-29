package llmreviewer

import (
	"context"
	"errors"
	"strings"
	"testing"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/capability-core/internal/learning"
	"google.golang.org/grpc"
)

// fakeInferer records the request and returns a canned response/error.
type fakeInferer struct {
	got  *mpv1.InferRequest
	resp *mpv1.InferResponse
	err  error
}

func (f *fakeInferer) Infer(
	_ context.Context,
	in *mpv1.InferRequest,
	_ ...grpc.CallOption,
) (*mpv1.InferResponse, error) {
	f.got = in
	return f.resp, f.err
}

func TestReview_ParsesCandidatesAndBuildsRequest(t *testing.T) {
	fake := &fakeInferer{
		resp: &mpv1.InferResponse{
			Content: `{"skills":[{"name":"Cache Tips","content":"use cache","confidence":0.8}]}`,
		},
	}
	r := NewReviewer(fake, "", "org-1") // empty model -> DefaultModel
	existing := []learning.ExistingSkill{{Name: "Runbook"}}

	got, err := r.Review(context.Background(), "did a thing", existing, "SYSTEM PROMPT")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(got) != 1 || got[0].Name != "Cache Tips" {
		t.Fatalf("bad candidates: %+v", got)
	}
	// A hostile/loose reply still can't mint a user skill.
	if got[0].Origin != learning.OriginBackgroundReview {
		t.Fatalf("origin must be forced to background_review: %+v", got[0])
	}

	// Request shape.
	if fake.got.GetOrgId() != "org-1" {
		t.Fatalf("org_id not propagated: %q", fake.got.GetOrgId())
	}
	if fake.got.GetModel() != DefaultModel {
		t.Fatalf("empty model should default to %q, got %q", DefaultModel, fake.got.GetModel())
	}
	if !fake.got.GetZdr() {
		t.Fatal("learning review must set ZDR (transcript may be sensitive)")
	}
	msgs := fake.got.GetMessages()
	if len(msgs) != 2 || msgs[0].GetRole() != "system" || msgs[1].GetRole() != "user" {
		t.Fatalf("expected system+user messages, got %+v", msgs)
	}
	if msgs[0].GetContent() != "SYSTEM PROMPT" {
		t.Fatalf("system message must carry the prompt, got %q", msgs[0].GetContent())
	}
	if !strings.Contains(msgs[1].GetContent(), "Runbook") {
		t.Fatal("existing skill names should appear in the user message (dedup hint)")
	}
	if !strings.Contains(msgs[1].GetContent(), "did a thing") {
		t.Fatal("transcript should appear in the user message")
	}
}

func TestReview_InferErrorIsWrapped(t *testing.T) {
	fake := &fakeInferer{err: errors.New("provider down")}
	r := NewReviewer(fake, "m", "org-1")
	_, err := r.Review(context.Background(), "t", nil, "p")
	if err == nil || !strings.Contains(err.Error(), "infer call") {
		t.Fatalf("expected wrapped infer error, got %v", err)
	}
}

func TestReview_EmptyResponseErrors(t *testing.T) {
	fake := &fakeInferer{resp: &mpv1.InferResponse{Content: "   "}}
	r := NewReviewer(fake, "m", "org-1")
	if _, err := r.Review(context.Background(), "t", nil, "p"); err == nil {
		t.Fatal("empty inference content must error")
	}
}

func TestReview_NilClientErrors(t *testing.T) {
	r := &Reviewer{} // nil client
	if _, err := r.Review(context.Background(), "t", nil, "p"); err == nil {
		t.Fatal("nil client must error")
	}
}

func TestReview_MalformedModelReplyPropagatesParseError(t *testing.T) {
	// A reply with no JSON object must surface ParseReviewResponse's error,
	// not silently yield zero candidates.
	fake := &fakeInferer{resp: &mpv1.InferResponse{Content: "the model just wrote prose"}}
	r := NewReviewer(fake, "m", "org-1")
	if _, err := r.Review(context.Background(), "t", nil, "p"); err == nil {
		t.Fatal("a reply with no JSON object must error")
	}
}
