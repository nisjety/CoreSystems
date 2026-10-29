package evaloptimizer

import (
	"errors"
	"testing"
)

func TestParseVerdict(t *testing.T) {
	tests := []struct {
		name         string
		content      string
		wantErr      bool
		wantPassed   bool
		wantScore    float64
		wantFeedback string
	}{
		{
			name:         "clean json",
			content:      `{"passed": true, "score": 0.9, "feedback": "solid"}`,
			wantPassed:   true,
			wantScore:    0.9,
			wantFeedback: "solid",
		},
		{
			name:         "fenced json with prose",
			content:      "Here is my grade:\n```json\n{\"passed\": false, \"score\": 0.3, \"feedback\": \"too short\"}\n```\nHope that helps.",
			wantPassed:   false,
			wantScore:    0.3,
			wantFeedback: "too short",
		},
		{
			name:       "score above 1 is clamped",
			content:    `{"passed": true, "score": 4.5, "feedback": "x"}`,
			wantPassed: true,
			wantScore:  1,
		},
		{
			name:       "negative score is clamped",
			content:    `{"passed": false, "score": -2, "feedback": "x"}`,
			wantPassed: false,
			wantScore:  0,
		},
		{
			name:       "missing passed defaults to false (fail closed)",
			content:    `{"score": 0.99, "feedback": "looks good"}`,
			wantPassed: false,
			wantScore:  0.99,
		},
		{
			name:       "braces inside string do not confuse the extractor",
			content:    `prefix {"passed": true, "score": 1, "feedback": "use {curly} carefully"} suffix`,
			wantPassed: true,
			wantScore:  1,
		},
		{
			name:    "no json object",
			content: "the answer is acceptable",
			wantErr: true,
		},
		{
			name:    "empty",
			content: "",
			wantErr: true,
		},
		{
			name:    "malformed json object",
			content: `{"passed": tru`,
			wantErr: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			v, err := ParseVerdict(tt.content)
			if tt.wantErr {
				if err == nil {
					t.Fatalf("expected error, got verdict %+v", v)
				}
				return
			}
			if err != nil {
				t.Fatalf("unexpected error: %v", err)
			}
			if v.Passed != tt.wantPassed {
				t.Errorf("Passed = %v, want %v", v.Passed, tt.wantPassed)
			}
			if v.Score != tt.wantScore {
				t.Errorf("Score = %v, want %v", v.Score, tt.wantScore)
			}
			if tt.wantFeedback != "" && v.Feedback != tt.wantFeedback {
				t.Errorf("Feedback = %q, want %q", v.Feedback, tt.wantFeedback)
			}
		})
	}
}

func TestParseVerdict_NoJSONErrorIsTyped(t *testing.T) {
	_, err := ParseVerdict("nope")
	if !errors.Is(err, ErrNoVerdictJSON) {
		t.Fatalf("want ErrNoVerdictJSON, got %v", err)
	}
}
