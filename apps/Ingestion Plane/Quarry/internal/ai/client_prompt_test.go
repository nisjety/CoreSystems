package ai

import "testing"

func TestComposeExtractPrompt(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name         string
		systemPrompt string
		prompt       string
		want         string
	}{
		{
			name:         "system and user prompt",
			systemPrompt: "Return strict JSON only.",
			prompt:       "extract the title",
			want:         "System instructions:\nReturn strict JSON only.\n\nUser request:\nextract the title",
		},
		{
			name:         "system prompt only",
			systemPrompt: "Return strict JSON only.",
			want:         "System instructions:\nReturn strict JSON only.",
		},
		{
			name:   "user prompt only",
			prompt: "extract the title",
			want:   "extract the title",
		},
		{
			name: "empty prompts",
			want: "",
		},
	}

	for _, tt := range tests {
		tt := tt
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			got := composeExtractPrompt(tt.systemPrompt, tt.prompt)
			if got != tt.want {
				t.Fatalf("composeExtractPrompt() = %q, want %q", got, tt.want)
			}
		})
	}
}
