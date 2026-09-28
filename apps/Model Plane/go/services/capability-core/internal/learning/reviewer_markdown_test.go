package learning

import (
	"encoding/json"
	"errors"
	"strings"
	"testing"
	"time"
)

// reviewArgs builds the compact JSON inference-core hands back as the
// submit_skill_review tool-call arguments (serde_json::to_string of the tool
// input): backticks and braces inside a skill's content stay literal.
func reviewArgs(t *testing.T, content string) string {
	t.Helper()
	b, err := json.Marshal(map[string]any{"skills": []any{map[string]any{
		"name":             "Build the edge image",
		"description":      "How to build it",
		"content":          content,
		"trigger_keywords": []string{"docker", "build"},
		"confidence":       0.8,
	}}})
	if err != nil {
		t.Fatal(err)
	}
	return string(b)
}

// Markdown is the natural format of a procedural skill, so a skill's content
// routinely carries fences and braces. The old extractor cut the reply at the
// first ``` anywhere, including inside a JSON string, and every one of these
// was rejected, retried four times and dropped.
var markdownSkillContents = []struct{ name, content string }{
	{"fenced bash block", "Steps:\n```bash\ndocker build -f deploy/compose/Dockerfile.edge --target build .\n```\nThen verify."},
	{"single inline triple backtick", "Wrap commands in ``` when documenting."},
	{"fenced json example", "Emit:\n```json\n{\"ok\":true}\n```\nDone."},
	{"brace text", "Interpolate {x} and {{name}} and close with }."},
}

func TestParseReviewResponse_MarkdownInSkillContentParses(t *testing.T) {
	for _, tc := range markdownSkillContents {
		t.Run(tc.name, func(t *testing.T) {
			got, err := ParseReviewResponse(reviewArgs(t, tc.content))
			if err != nil || len(got) != 1 {
				t.Fatalf("want 1 candidate, got %d err=%v", len(got), err)
			}
			if got[0].Content != tc.content {
				t.Fatalf("content altered:\n got: %q\nwant: %q", got[0].Content, tc.content)
			}
		})
	}
}

func TestParseReviewResponse_FencedReplyWithMarkdownContentParses(t *testing.T) {
	// The text path: a fenced reply whose skill content has its own fence.
	for _, tc := range markdownSkillContents {
		t.Run(tc.name, func(t *testing.T) {
			raw := "Here is the review:\n\n```json\n" + reviewArgs(t, tc.content) + "\n```\nDone."
			got, err := ParseReviewResponse(raw)
			if err != nil || len(got) != 1 || got[0].Content != tc.content {
				t.Fatalf("want the 1 candidate intact, got %+v err=%v", got, err)
			}
		})
	}
}

func TestParseReviewResponse_ProseWithBracesBeforeObject(t *testing.T) {
	raw := "I considered {x} and an example {\"a\":1} first.\n" +
		`{"skills":[{"name":"X","content":"c","confidence":0.6}]}` +
		"\nAnd a trailing {y}."
	got, err := ParseReviewResponse(raw)
	if err != nil || len(got) != 1 || got[0].Name != "X" {
		t.Fatalf("object after brace prose not found: %+v err=%v", got, err)
	}
}

func TestParseReviewResponse_TruncatedAndEmptyObjectsStillRejected(t *testing.T) {
	full := reviewArgs(t, "Steps:\n```bash\nmake\n```")
	for _, raw := range []string{
		full[:len(full)-3],
		"```json\n" + full[:len(full)/2],
		`{}`,
		`{"skills":{}}`,
		`{"skills":[{"name":"X","content":"c","confidence":"high"}]}`,
	} {
		if got, err := ParseReviewResponse(raw); err == nil {
			t.Fatalf("accepted invalid review %q: %+v", raw, got)
		}
	}
}

func TestParseReviewJSON_ToolArgumentsWithMarkdownContentParse(t *testing.T) {
	for _, tc := range markdownSkillContents {
		t.Run(tc.name, func(t *testing.T) {
			got, err := ParseReviewJSON(reviewArgs(t, tc.content))
			if err != nil || len(got) != 1 || got[0].Content != tc.content {
				t.Fatalf("want the 1 candidate intact, got %+v err=%v", got, err)
			}
			if got[0].Origin != OriginBackgroundReview {
				t.Fatalf("origin must be forced to background_review: %+v", got[0])
			}
		})
	}
}

func TestParseReviewJSON_IsStrict(t *testing.T) {
	// Tool arguments are one object; text-style repair has no place there.
	valid := reviewArgs(t, "plain steps")
	for _, raw := range []string{
		"```json\n" + valid + "\n```",
		"Here it is: " + valid,
		valid + " trailing prose",
		"[" + valid + "]",
	} {
		if got, err := ParseReviewJSON(raw); err == nil {
			t.Fatalf("strict parser accepted %q: %+v", raw, got)
		}
	}
	if got, err := ParseReviewJSON("\n  " + valid + "\n"); err != nil || len(got) != 1 {
		t.Fatalf("surrounding whitespace is still one object: %+v err=%v", got, err)
	}
}

func TestParseErrors_CarryReasonAndNoContent(t *testing.T) {
	const secret = "customer-secret-7731"
	nine := `{"skills":[` + strings.TrimSuffix(strings.Repeat(`{"name":"n","content":"c","confidence":0.9},`, 9), ",") + `]}`
	for _, tc := range []struct {
		name  string
		parse func(string) ([]SkillCandidate, error)
		raw   string
		want  ParseReason
	}{
		{"prose only", ParseReviewResponse, "the model wrote " + secret, ReasonNoJSON},
		{"empty object", ParseReviewResponse, `{"note":"` + secret + `"}`, ReasonNoJSON},
		{"truncated text", ParseReviewResponse, `{"skills":[{"name":"` + secret, ReasonSyntax},
		{"truncated tool", ParseReviewJSON, `{"skills":[{"name":"` + secret, ReasonSyntax},
		{"not an object", ParseReviewJSON, `["` + secret + `"]`, ReasonNoJSON},
		{"skills null", ParseReviewJSON, `{"skills":null,"x":"` + secret + `"}`, ReasonMissingSkills},
		{"skills absent", ParseReviewJSON, `{"x":"` + secret + `"}`, ReasonMissingSkills},
		{"skills not array", ParseReviewJSON, `{"skills":"` + secret + `"}`, ReasonMissingSkills},
		{"candidate missing confidence", ParseReviewJSON, `{"skills":[{"name":"` + secret + `","content":"c"}]}`, ReasonCandidateInvalid},
		{"candidate wrong type", ParseReviewJSON, `{"skills":[{"name":"x","content":"` + secret + `","confidence":"high"}]}`, ReasonCandidateInvalid},
		{"too many skills", ParseReviewJSON, nine, ReasonSize},
		{"reply too large", ParseReviewResponse, secret + strings.Repeat(" ", maxReviewResponseBytes), ReasonSize},
	} {
		t.Run(tc.name, func(t *testing.T) {
			_, err := tc.parse(tc.raw)
			var parseErr *ParseError
			if !errors.As(err, &parseErr) || parseErr.Reason != tc.want {
				t.Fatalf("err = %v, want ParseError reason %s", err, tc.want)
			}
			if strings.Contains(err.Error(), secret) {
				t.Fatalf("error quotes the reply: %v", err)
			}
		})
	}
}

func TestParseReviewResponse_ScanIsBoundedOnUnclosedNesting(t *testing.T) {
	// Every '{' restarts a decode that runs to the end of this text. Without
	// the scan budget the 64 KiB case took seconds.
	raw := strings.Repeat(`{"a":`, maxReviewResponseBytes/5)
	start := time.Now()
	if _, err := ParseReviewResponse(raw); err == nil {
		t.Fatal("unclosed nesting must not parse")
	}
	if elapsed := time.Since(start); elapsed > 2*time.Second {
		t.Fatalf("scan took %s on %d bytes; it must stay bounded", elapsed, len(raw))
	}
}
