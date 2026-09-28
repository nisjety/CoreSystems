package learning

import "testing"

func TestParseReviewResponse_CleanJSON(t *testing.T) {
	raw := `{"skills":[{"name":"Cache Tips","description":"d","content":"use cache","trigger_keywords":["cache","perf"],"confidence":0.8}]}`
	got, err := ParseReviewResponse(raw)
	if err != nil || len(got) != 1 {
		t.Fatalf("expected 1 candidate, got %d err=%v", len(got), err)
	}
	c := got[0]
	if c.Name != "Cache Tips" || c.Content != "use cache" || c.Confidence != 0.8 {
		t.Fatalf("bad mapping: %+v", c)
	}
	if len(c.TriggerKeywords) != 2 || c.Origin != OriginBackgroundReview {
		t.Fatalf("triggers/origin wrong: %+v", c)
	}
}

func TestParseReviewResponse_FencedAndProse(t *testing.T) {
	raw := "Here are the skills I propose:\n\n```json\n{\"skills\":[{\"name\":\"X\",\"content\":\"c\",\"confidence\":0.6}]}\n```\nLet me know!"
	got, err := ParseReviewResponse(raw)
	if err != nil || len(got) != 1 || got[0].Name != "X" {
		t.Fatalf("fenced+prose extraction failed: %+v err=%v", got, err)
	}
}

func TestParseReviewResponse_EmptySkills(t *testing.T) {
	got, err := ParseReviewResponse(`{"skills":[]}`)
	if err != nil || len(got) != 0 {
		t.Fatalf("empty skills should yield 0 candidates, got %d err=%v", len(got), err)
	}
}

func TestParseReviewResponse_Malformed(t *testing.T) {
	if _, err := ParseReviewResponse("the model refused and wrote only prose"); err == nil {
		t.Fatal("no JSON object must error")
	}
	if _, err := ParseReviewResponse(`{"skills": [ broken `); err == nil {
		t.Fatal("malformed JSON must error")
	}
}

func TestParseReviewResponse_ForcesBackgroundReviewOrigin(t *testing.T) {
	// Even if the model tries to claim origin, candidates are background_review.
	raw := `{"skills":[{"name":"Y","content":"c","confidence":0.9}]}`
	got, _ := ParseReviewResponse(raw)
	if len(got) != 1 || got[0].Origin != OriginBackgroundReview {
		t.Fatalf("origin must be forced to background_review: %+v", got)
	}
}

func TestParseReviewResponse_RejectsMissingAndInvalidSchema(t *testing.T) {
	for _, raw := range []string{`{}`, `{"skills":null}`, `{"skills":[{"name":"X","content":"c"}]}`, `{"skills":[{"name":"X","content":"c","confidence":1.5}]}`, `{"skills":[{"name":"","content":"c","confidence":0.9}]}`} {
		if _, err := ParseReviewResponse(raw); err == nil {
			t.Fatalf("accepted invalid review: %s", raw)
		}
	}
}
