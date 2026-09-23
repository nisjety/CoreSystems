package emailsync

import (
	"encoding/json"
	"strings"
	"testing"
)

// normalizeGraphJSON builds a graphMessage the way the fetcher does -- from
// Graph's own JSON -- so these tests exercise the real decode path.
func normalizeGraphJSON(t *testing.T, payload string) EmailMessage {
	t.Helper()
	var message graphMessage
	if err := json.Unmarshal([]byte(payload), &message); err != nil {
		t.Fatalf("unmarshal graph message: %v", err)
	}
	normalized, ok := message.normalize()
	if !ok {
		t.Fatalf("normalize() refused a valid message: %s", payload)
	}
	return normalized
}

// The bug this covers: Graph caps bodyPreview at 255 characters, and the mail
// path stored that field as body_text for every HTML message. Nothing
// downstream could tell a truncated preview from a genuinely short email.
func TestGraphHTMLBodyIsRenderedInFullNotTruncatedToThePreview(t *testing.T) {
	sentence := strings.Repeat("Aquatiq leverer hygieneutstyr til norsk naeringsmiddelindustri. ", 20)
	payload, err := json.Marshal(map[string]any{
		"id":               "m1",
		"receivedDateTime": "2026-09-14T10:00:00Z",
		"bodyPreview":      sentence[:255],
		"body":             map[string]any{"contentType": "html", "content": "<html><body><p>" + sentence + "</p></body></html>"},
		"from":             map[string]any{"emailAddress": map[string]any{"address": "kunde@example.no"}},
	})
	if err != nil {
		t.Fatal(err)
	}

	normalized := normalizeGraphJSON(t, string(payload))

	if len(normalized.BodyText) <= 255 {
		t.Fatalf("body_text is %d chars, want the full rendered body (>255)", len(normalized.BodyText))
	}
	if !strings.Contains(normalized.BodyText, "naeringsmiddelindustri") {
		t.Fatalf("body_text lost the message content: %q", normalized.BodyText)
	}
	if strings.Contains(normalized.BodyText, "<p>") {
		t.Fatalf("body_text still carries markup: %q", normalized.BodyText)
	}
	if normalized.BodyHTML == "" {
		t.Fatal("body_html must still carry the original markup")
	}
}

// Outlook mail is mostly <style>. Stripping the tags but keeping their
// contents would replace a 255-character preview with a wall of CSS -- a
// different failure, not a fix.
func TestGraphHTMLBodyDropsStyleAndScriptContent(t *testing.T) {
	payload, err := json.Marshal(map[string]any{
		"id":               "m2",
		"receivedDateTime": "2026-09-14T10:00:00Z",
		"body": map[string]any{
			"contentType": "html",
			"content":     "<html><head><style>.x{color:red}</style></head><body><script>var a=1;</script><p>Hei</p><p>Ha det</p></body></html>",
		},
		"from": map[string]any{"emailAddress": map[string]any{"address": "kunde@example.no"}},
	})
	if err != nil {
		t.Fatal(err)
	}

	normalized := normalizeGraphJSON(t, string(payload))

	for _, unwanted := range []string{"color:red", "var a=1", ".x{"} {
		if strings.Contains(normalized.BodyText, unwanted) {
			t.Errorf("body_text leaked %q from a non-text element: %q", unwanted, normalized.BodyText)
		}
	}
	// Adjacent blocks must not be welded into one word.
	if normalized.BodyText != "Hei Ha det" {
		t.Errorf("body_text = %q, want %q", normalized.BodyText, "Hei Ha det")
	}
}

// A body that renders to nothing (an image-only mail) has no text to lose, so
// the preview is the best available value rather than a truncation of one.
func TestGraphHTMLBodyFallsBackToPreviewWhenMarkupRendersEmpty(t *testing.T) {
	payload, err := json.Marshal(map[string]any{
		"id":               "m3",
		"receivedDateTime": "2026-09-14T10:00:00Z",
		"bodyPreview":      "Se vedlagt bilde",
		"body":             map[string]any{"contentType": "html", "content": `<html><body><img src="cid:x"></body></html>`},
		"from":             map[string]any{"emailAddress": map[string]any{"address": "kunde@example.no"}},
	})
	if err != nil {
		t.Fatal(err)
	}

	normalized := normalizeGraphJSON(t, string(payload))

	if normalized.BodyText != "Se vedlagt bilde" {
		t.Errorf("body_text = %q, want the preview as the fallback", normalized.BodyText)
	}
}

func TestStripHTMLTagsBoundaries(t *testing.T) {
	// Everything after an unterminated <style> would have rendered as CSS in a
	// browser. Carrying it into body_text would invent content, not recover it.
	if got := stripHTMLTags("<p>Hei</p><style>.a{x:1}"); got != "Hei" {
		t.Errorf("unterminated style: stripHTMLTags() = %q, want %q", got, "Hei")
	}
	// A tag whose name merely STARTS with a dropped name must survive.
	if got := stripHTMLTags("<strong>Viktig</strong>"); got != "Viktig" {
		t.Errorf("prefix collision: stripHTMLTags() = %q, want %q", got, "Viktig")
	}
	// Entities are message text, not markup.
	if got := stripHTMLTags("<p>Kari &amp; Ola</p>"); got != "Kari & Ola" {
		t.Errorf("entities: stripHTMLTags() = %q, want %q", got, "Kari & Ola")
	}
	// `<head` is a prefix of `<header`. A near-miss must not end the scan and
	// leave a later, genuine <head> in place.
	if got := stripHTMLTags("<header>Topp</header><head><style>.a{x:1}</style></head><p>Brod</p>"); got != "Topp Brod" {
		t.Errorf("near-miss tag name: stripHTMLTags() = %q, want %q", got, "Topp Brod")
	}
}
