package llmreviewer

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/capability-core/internal/learning"
	"google.golang.org/grpc"
	"google.golang.org/grpc/metadata"
)

// metadataInferer records the outgoing gRPC metadata of the review call.
type metadataInferer struct {
	md   metadata.MD
	resp *mpv1.InferResponse
}

func (f *metadataInferer) Infer(ctx context.Context, _ *mpv1.InferRequest, _ ...grpc.CallOption) (*mpv1.InferResponse, error) {
	f.md, _ = metadata.FromOutgoingContext(ctx)
	return f.resp, nil
}

// toolReply is a forced submit_skill_review reply the way inference-core
// returns it: compact JSON arguments with the skill content verbatim.
func toolReply(t *testing.T, name, content string) *mpv1.InferResponse {
	t.Helper()
	args, err := json.Marshal(map[string]any{"skills": []any{map[string]any{
		"name": name, "description": "d", "content": content,
		"trigger_keywords": []string{"build"}, "confidence": 0.8,
	}}})
	if err != nil {
		t.Fatal(err)
	}
	return &mpv1.InferResponse{
		ModelUsed: "claude-sonnet-4-5", StopReason: "tool_use", OutputTokens: 180,
		ToolCalls: []*mpv1.ToolCall{{Id: "toolu_1", Name: reviewTool, ArgumentsJson: string(args)}},
	}
}

func TestReview_FencedSkillContentInToolCallYieldsOneCandidate(t *testing.T) {
	content := "Steps:\n```bash\ndocker build -f deploy/compose/Dockerfile.edge --target build .\n```\nThen verify."
	fake := &fakeInferer{resp: toolReply(t, "Build the edge image", content)}
	got, err := NewReviewer(fake, "m", "org-1").Review(context.Background(), "t", nil, learning.ReviewPrompt)
	if err != nil || len(got) != 1 || got[0].Content != content {
		t.Fatalf("want 1 intact candidate, got %+v err=%v", got, err)
	}
}

func TestReview_ToolCallTakesPrecedenceOverReplyText(t *testing.T) {
	resp := toolReply(t, "From Tool", "tool content")
	resp.Content = `{"skills":[{"name":"From Text","content":"text content","confidence":0.9}]}`
	got, err := NewReviewer(&fakeInferer{resp: resp}, "m", "org-1").Review(context.Background(), "t", nil, "p")
	if err != nil || len(got) != 1 || got[0].Name != "From Tool" {
		t.Fatalf("tool call must win over reply text: %+v err=%v", got, err)
	}

	// A broken tool call is not rescued by valid-looking reply text: the
	// arguments are the answer, parsed strictly.
	resp = toolReply(t, "From Tool", "tool content")
	resp.ToolCalls[0].ArgumentsJson = "```json\n" + resp.ToolCalls[0].ArgumentsJson + "\n```"
	resp.Content = `{"skills":[{"name":"From Text","content":"text content","confidence":0.9}]}`
	_, err = NewReviewer(&fakeInferer{resp: resp}, "m", "org-1").Review(context.Background(), "t", nil, "p")
	var typed *ResponseError
	if !errors.As(err, &typed) || typed.Source != "tool" || typed.Reason != learning.ReasonSyntax {
		t.Fatalf("fenced tool arguments must fail as tool/syntax, got %v", err)
	}
}

func TestReview_ParseFailureCarriesReasonSourceAndLengthButNoContent(t *testing.T) {
	const secret = "customer-secret-7731"
	for _, tc := range []struct {
		name       string
		resp       *mpv1.InferResponse
		wantSource string
		wantReason learning.ParseReason
	}{
		{"text prose", &mpv1.InferResponse{Content: "I would suggest " + secret}, "text", learning.ReasonNoJSON},
		{"text truncated", &mpv1.InferResponse{Content: `{"skills":[{"name":"` + secret}, "text", learning.ReasonSyntax},
		{"tool missing skills", &mpv1.InferResponse{StopReason: "tool_use", ToolCalls: []*mpv1.ToolCall{{Name: reviewTool, ArgumentsJson: `{"note":"` + secret + `"}`}}}, "tool", learning.ReasonMissingSkills},
		{"tool bad candidate", &mpv1.InferResponse{StopReason: "tool_use", ToolCalls: []*mpv1.ToolCall{{Name: reviewTool, ArgumentsJson: `{"skills":[{"name":"` + secret + `"}]}`}}}, "tool", learning.ReasonCandidateInvalid},
	} {
		t.Run(tc.name, func(t *testing.T) {
			_, err := NewReviewer(&fakeInferer{resp: tc.resp}, "m", "org-1").Review(context.Background(), secret, nil, "p")
			var typed *ResponseError
			if !errors.As(err, &typed) || typed.Kind != "invalid_schema" {
				t.Fatalf("want a typed invalid_schema failure, got %v", err)
			}
			raw := tc.resp.GetContent()
			if calls := tc.resp.GetToolCalls(); len(calls) > 0 {
				raw = calls[0].GetArgumentsJson()
			}
			if typed.Source != tc.wantSource || typed.Reason != tc.wantReason || typed.RawLength != len(raw) {
				t.Fatalf("diagnostics = source %q reason %q raw_len %d, want %q %q %d",
					typed.Source, typed.Reason, typed.RawLength, tc.wantSource, tc.wantReason, len(raw))
			}
			for _, part := range []string{"reason=" + string(tc.wantReason), "source=" + tc.wantSource} {
				if !strings.Contains(err.Error(), part) {
					t.Fatalf("error %q lacks %q", err, part)
				}
			}
			if strings.Contains(err.Error(), secret) {
				t.Fatalf("error leaks reply or transcript content: %v", err)
			}
		})
	}
}

func TestReview_BypassesInferenceResponseCache(t *testing.T) {
	fake := &metadataInferer{resp: &mpv1.InferResponse{Content: `{"skills":[]}`}}
	if _, err := NewReviewer(fake, "m", "org-1").Review(context.Background(), "t", nil, "p"); err != nil {
		t.Fatal(err)
	}
	if got := fake.md.Get("cache-control"); len(got) != 1 || got[0] != "no-store" {
		t.Fatalf("cache-control metadata = %q, want exactly no-store", got)
	}
}
