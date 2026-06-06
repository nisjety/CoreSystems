package news

import "testing"

func TestCleanTextStripsHTMLAndTruncates(t *testing.T) {
	input := "<p>Hello <strong>world</strong></p>"
	if got := cleanText(input); got != "Hello world" {
		t.Fatalf("cleanText() = %q", got)
	}
}

func TestNormalizePubDateFallsBackToRFC3339(t *testing.T) {
	got := normalizePubDate("Fri, 05 Jun 2026 09:00:00 GMT")
	if got != "2026-06-05T09:00:00Z" {
		t.Fatalf("normalizePubDate() = %q", got)
	}
}
