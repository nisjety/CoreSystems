package evaloptimizer

import (
	"encoding/json"
	"errors"
	"fmt"
	"strings"
)

// VerdictSchema is the JSON Schema handed to the judge as a
// StructuredOutputSchema so providers that support structured output emit a
// clean, parseable verdict. It is intentionally small: the three fields the
// loop reasons about.
const VerdictSchema = `{
  "type": "object",
  "properties": {
    "passed": {"type": "boolean", "description": "true only if the answer satisfies the rubric"},
    "score": {"type": "number", "description": "quality score in [0,1] against the rubric"},
    "feedback": {"type": "string", "description": "concrete, actionable guidance for the next revision"}
  },
  "required": ["passed", "score", "feedback"],
  "additionalProperties": false
}`

// ErrNoVerdictJSON is returned when no JSON object can be located in the
// judge's response.
var ErrNoVerdictJSON = errors.New("evaloptimizer: no JSON object found in judge response")

// rawVerdict mirrors VerdictSchema for decoding. Kept separate from the public
// Verdict so parsing stays tolerant of missing/extra fields.
type rawVerdict struct {
	Passed   *bool    `json:"passed"`
	Score    *float64 `json:"score"`
	Feedback string   `json:"feedback"`
}

// ParseVerdict extracts a structured Verdict from a judge model's raw text.
//
// It is deliberately forgiving: providers vary in whether they honour a
// structured-output schema, and some wrap JSON in Markdown code fences or
// surround it with prose. The first balanced JSON object in the string is
// decoded. The score is clamped to [0,1] so a mis-scaled judge cannot push a
// non-passing answer over a threshold. A missing "passed" field defaults to
// false (fail-closed): the loop must never treat an unparseable grade as a
// pass.
func ParseVerdict(content string) (Verdict, error) {
	obj, ok := extractJSONObject(content)
	if !ok {
		return Verdict{}, fmt.Errorf("%w: %q", ErrNoVerdictJSON, truncate(content, 120))
	}
	var rv rawVerdict
	if err := json.Unmarshal([]byte(obj), &rv); err != nil {
		return Verdict{}, fmt.Errorf("evaloptimizer: decode verdict: %w", err)
	}
	v := Verdict{Feedback: strings.TrimSpace(rv.Feedback)}
	if rv.Passed != nil {
		v.Passed = *rv.Passed
	}
	if rv.Score != nil {
		v.Score = clamp01(*rv.Score)
	}
	return v, nil
}

// extractJSONObject returns the first balanced, top-level JSON object embedded
// in s (ignoring braces inside string literals) and reports whether one was
// found. This handles fenced blocks and leading/trailing prose without a
// regexp.
func extractJSONObject(s string) (string, bool) {
	start := strings.IndexByte(s, '{')
	if start < 0 {
		return "", false
	}
	depth := 0
	inString := false
	escaped := false
	for i := start; i < len(s); i++ {
		c := s[i]
		if inString {
			switch {
			case escaped:
				escaped = false
			case c == '\\':
				escaped = true
			case c == '"':
				inString = false
			}
			continue
		}
		switch c {
		case '"':
			inString = true
		case '{':
			depth++
		case '}':
			depth--
			if depth == 0 {
				return s[start : i+1], true
			}
		}
	}
	return "", false
}

func clamp01(f float64) float64 {
	if f < 0 {
		return 0
	}
	if f > 1 {
		return 1
	}
	return f
}

func truncate(s string, n int) string {
	s = strings.TrimSpace(s)
	if len(s) <= n {
		return s
	}
	return s[:n] + "…"
}
