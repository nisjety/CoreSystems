package learning

import (
	"encoding/json"
	"fmt"
	"strings"
)

// ParseReviewResponse turns an LLM review's raw text into typed skill
// candidates. This is the risky, testable half of the G7 Reviewer: the model
// emits untrusted text, and we must extract well-formed candidates from it.
// The actual model call (inference-core) is thin glue layered on top.
//
// The model is asked (via ReviewPrompt) to emit JSON shaped as:
//
//	{"skills":[{"name","description","content","trigger_keywords":[...],"confidence":0.0-1.0}]}
//
// Real models routinely wrap JSON in ```json fences or surround it with prose;
// [extractJSONObject] tolerates both. Every returned candidate is forced to
// Origin=background_review (the producer can never mint a "user" skill).
func ParseReviewResponse(raw string) ([]SkillCandidate, error) {
	js := extractJSONObject(raw)
	if js == "" {
		return nil, fmt.Errorf("review response contained no JSON object")
	}
	var doc struct {
		Skills []struct {
			Name            string   `json:"name"`
			Description     string   `json:"description"`
			Content         string   `json:"content"`
			TriggerKeywords []string `json:"trigger_keywords"`
			Confidence      float64  `json:"confidence"`
		} `json:"skills"`
	}
	if err := json.Unmarshal([]byte(js), &doc); err != nil {
		return nil, fmt.Errorf("parse review response: %w", err)
	}
	out := make([]SkillCandidate, 0, len(doc.Skills))
	for _, s := range doc.Skills {
		out = append(out, SkillCandidate{
			Name:            strings.TrimSpace(s.Name),
			Description:     s.Description,
			Content:         s.Content,
			TriggerKeywords: s.TriggerKeywords,
			Confidence:      s.Confidence,
			Origin:          OriginBackgroundReview,
		})
	}
	return out, nil
}

// extractJSONObject pulls the JSON object out of model text: strips a leading
// ```json (or ```) fence if present, then takes the span from the first '{'
// to the last '}'. Returns "" if no object-looking span is found.
func extractJSONObject(raw string) string {
	s := strings.TrimSpace(raw)
	if i := strings.Index(s, "```"); i >= 0 {
		s = s[i+3:]
		s = strings.TrimPrefix(s, "json")
		s = strings.TrimPrefix(s, "JSON")
		if j := strings.Index(s, "```"); j >= 0 {
			s = s[:j]
		}
		s = strings.TrimSpace(s)
	}
	a := strings.Index(s, "{")
	b := strings.LastIndex(s, "}")
	if a < 0 || b <= a {
		return ""
	}
	return s[a : b+1]
}
