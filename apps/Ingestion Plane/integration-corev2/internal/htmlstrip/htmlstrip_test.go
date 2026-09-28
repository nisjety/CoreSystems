package htmlstrip

import "testing"

// These pin the exact behavior lifted from internal/emailsync's
// graph_body_test.go — the move must be behavior-preserving, not a rewrite.
func TestStripHTMLTagsBoundaries(t *testing.T) {
	if got := StripHTMLTags("<p>Hei</p><style>.a{x:1}"); got != "Hei" {
		t.Errorf("unterminated style: StripHTMLTags() = %q, want %q", got, "Hei")
	}

	if got := StripHTMLTags("<strong>Viktig</strong>"); got != "Viktig" {
		t.Errorf("prefix collision: StripHTMLTags() = %q, want %q", got, "Viktig")
	}

	if got := StripHTMLTags("<p>Kari &amp; Ola</p>"); got != "Kari & Ola" {
		t.Errorf("entities: StripHTMLTags() = %q, want %q", got, "Kari & Ola")
	}

	if got := StripHTMLTags("<header>Topp</header><head><style>.a{x:1}</style></head><p>Brod</p>"); got != "Topp Brod" {
		t.Errorf("near-miss tag name: StripHTMLTags() = %q, want %q", got, "Topp Brod")
	}
}

func TestStripHTMLTagsSeparatesAdjacentBlocks(t *testing.T) {
	if got := StripHTMLTags("<p>a</p><p>b</p>"); got != "a b" {
		t.Errorf("adjacent blocks must not concatenate: StripHTMLTags() = %q, want %q", got, "a b")
	}
}

func TestStripHTMLTagsHandlesScriptAndEmptyInput(t *testing.T) {
	if got := StripHTMLTags("<script>alert(1)</script>Hello"); got != "Hello" {
		t.Errorf("script element must be dropped: StripHTMLTags() = %q, want %q", got, "Hello")
	}
	if got := StripHTMLTags(""); got != "" {
		t.Errorf("empty input: StripHTMLTags() = %q, want empty string", got)
	}
}
