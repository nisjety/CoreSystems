package mailtask

import (
	"encoding/base64"
	"encoding/json"
	"strings"
	"testing"
)

// postmarkFixture builds a realistic Postmark inbound-webhook JSON payload
// (https://postmarkapp.com/support/article/1197-parse-an-inbound-email),
// with an overridable Authentication-Results header and attachment list.
func postmarkFixture(t *testing.T, authResultsHeader string, attachments []map[string]string) []byte {
	t.Helper()
	headers := []map[string]string{}
	if authResultsHeader != "" {
		headers = append(headers, map[string]string{"Name": "Authentication-Results", "Value": authResultsHeader})
	}
	payload := map[string]any{
		"MessageID":         "provider-msg-1",
		"From":              "owner@example.com",
		"FromFull":          map[string]string{"Email": "owner@example.com", "Name": "Task Owner"},
		"To":                "abc123def456@tasks.verevon.ai",
		"OriginalRecipient": "abc123def456@tasks.verevon.ai",
		"Subject":           "Please handle this",
		"TextBody":          "Body text",
		"HtmlBody":          "<p>Body text</p>",
		"Headers":           headers,
		"Attachments":       attachments,
	}
	body, err := json.Marshal(payload)
	if err != nil {
		t.Fatalf("marshal fixture: %v", err)
	}
	return body
}

func TestParsePostmarkInboundPayloadHappyPath(t *testing.T) {
	body := postmarkFixture(t, "mx.example.com; spf=pass smtp.mailfrom=owner@example.com; dkim=pass header.d=example.com", nil)

	event, err := ParsePostmarkInboundPayload(body)
	if err != nil {
		t.Fatalf("ParsePostmarkInboundPayload() error = %v", err)
	}
	if event.Provider != "postmark" {
		t.Fatalf("Provider = %q", event.Provider)
	}
	if event.ProviderMessageID != "provider-msg-1" {
		t.Fatalf("ProviderMessageID = %q", event.ProviderMessageID)
	}
	if event.Alias != "abc123def456" {
		t.Fatalf("Alias = %q", event.Alias)
	}
	if event.ToDomain != "tasks.verevon.ai" {
		t.Fatalf("ToDomain = %q", event.ToDomain)
	}
	if event.FromEmail != "owner@example.com" {
		t.Fatalf("FromEmail = %q", event.FromEmail)
	}
	if event.AuthResult != "pass" {
		t.Fatalf("AuthResult = %q, want pass", event.AuthResult)
	}
	if event.Subject != "Please handle this" {
		t.Fatalf("Subject = %q", event.Subject)
	}
}

func TestParsePostmarkInboundPayloadAuthResultVariants(t *testing.T) {
	tests := []struct {
		name   string
		header string
		want   string
	}{
		{name: "both pass", header: "spf=pass; dkim=pass", want: "pass"},
		{name: "spf fail", header: "spf=fail; dkim=pass", want: "fail"},
		{name: "dkim fail", header: "spf=pass; dkim=fail", want: "fail"},
		{name: "spf pass only, no dkim token", header: "spf=pass", want: "fail"},
		{name: "no header", header: "", want: "none"},
		{name: "header present but no tokens", header: "mx.example.com; unrelated=value", want: "none"},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			body := postmarkFixture(t, test.header, nil)
			event, err := ParsePostmarkInboundPayload(body)
			if err != nil {
				t.Fatalf("ParsePostmarkInboundPayload() error = %v", err)
			}
			if event.AuthResult != test.want {
				t.Fatalf("AuthResult = %q, want %q", event.AuthResult, test.want)
			}
		})
	}
}

// TestParsePostmarkInboundPayloadRejectsDuplicateAuthResultsHeaders covers
// the case where the raw message carries more than one Authentication-Results
// header -- e.g. a forger's own injected "spf=pass; dkim=pass" line ahead of
// (or alongside) whatever Postmark's own receiving MTA added. The parser must
// not silently trust either one; the whole point of the AuthResult=="pass"
// gate is defeated if a same-named duplicate can be picked over the genuine
// verdict.
func TestParsePostmarkInboundPayloadRejectsDuplicateAuthResultsHeaders(t *testing.T) {
	payload := map[string]any{
		"MessageID":         "provider-msg-dup",
		"From":              "owner@example.com",
		"FromFull":          map[string]string{"Email": "owner@example.com", "Name": "Task Owner"},
		"To":                "abc123def456@tasks.verevon.ai",
		"OriginalRecipient": "abc123def456@tasks.verevon.ai",
		"Subject":           "Please handle this",
		"TextBody":          "Body text",
		"HtmlBody":          "<p>Body text</p>",
		"Headers": []map[string]string{
			{"Name": "Authentication-Results", "Value": "forged-injected; spf=pass; dkim=pass"},
			{"Name": "Authentication-Results", "Value": "mx.example.com; spf=fail; dkim=fail"},
		},
	}
	body, err := json.Marshal(payload)
	if err != nil {
		t.Fatalf("marshal fixture: %v", err)
	}

	event, err := ParsePostmarkInboundPayload(body)
	if err != nil {
		t.Fatalf("ParsePostmarkInboundPayload() error = %v", err)
	}
	if event.AuthResult == "pass" {
		t.Fatalf("AuthResult = %q, want anything but pass when the header name is duplicated", event.AuthResult)
	}
}

func TestParsePostmarkInboundPayloadDecodesAttachments(t *testing.T) {
	content := base64.StdEncoding.EncodeToString([]byte("hello world"))
	body := postmarkFixture(t, "spf=pass; dkim=pass", []map[string]string{
		{"Name": "notes.txt", "Content": content, "ContentType": "text/plain"},
	})

	event, err := ParsePostmarkInboundPayload(body)
	if err != nil {
		t.Fatalf("ParsePostmarkInboundPayload() error = %v", err)
	}
	if len(event.Attachments) != 1 {
		t.Fatalf("Attachments = %d, want 1", len(event.Attachments))
	}
	got := event.Attachments[0]
	if got.Filename != "notes.txt" || got.ContentType != "text/plain" || string(got.Content) != "hello world" {
		t.Fatalf("attachment = %+v", got)
	}
}

func TestParsePostmarkInboundPayloadHandlesMalformedAttachmentEncodingWithoutFailingTheMessage(t *testing.T) {
	body := postmarkFixture(t, "spf=pass; dkim=pass", []map[string]string{
		{"Name": "broken.txt", "Content": "not-valid-base64!!", "ContentType": "text/plain"},
	})

	event, err := ParsePostmarkInboundPayload(body)
	if err != nil {
		t.Fatalf("ParsePostmarkInboundPayload() error = %v, want the message to still parse", err)
	}
	if len(event.Attachments) != 1 || len(event.Attachments[0].Content) != 0 {
		t.Fatalf("attachments = %+v, want one zero-byte attachment rather than a failure", event.Attachments)
	}
}

func TestParsePostmarkInboundPayloadRejectsOversizedBody(t *testing.T) {
	huge := make([]byte, maxInboundPayloadBytes+1)
	_, err := ParsePostmarkInboundPayload(huge)
	if err == nil {
		t.Fatal("expected an error for an oversized payload")
	}
}

func TestParsePostmarkInboundPayloadRejectsInvalidJSON(t *testing.T) {
	_, err := ParsePostmarkInboundPayload([]byte("not json"))
	if err == nil {
		t.Fatal("expected an error for invalid JSON")
	}
}

func TestSplitAliasAndDomainHandlesAngleBracketForm(t *testing.T) {
	alias, domain := splitAliasAndDomain("Task Inbox <abc123@tasks.verevon.ai>")
	if alias != "abc123" || domain != "tasks.verevon.ai" {
		t.Fatalf("alias/domain = %q/%q", alias, domain)
	}
}

func TestSplitAliasAndDomainRequiresAnAtSign(t *testing.T) {
	alias, domain := splitAliasAndDomain("not-an-email")
	if alias != "" || domain != "" {
		t.Fatalf("alias/domain = %q/%q, want both empty", alias, domain)
	}
}

func TestSplitAliasAndDomainTrimsWhitespace(t *testing.T) {
	alias, domain := splitAliasAndDomain("  abc123@tasks.verevon.ai  ")
	if alias != "abc123" || domain != "tasks.verevon.ai" {
		t.Fatalf("alias/domain = %q/%q", alias, domain)
	}
}

func TestParsePostmarkInboundPayloadLowercasesAliasAndDomain(t *testing.T) {
	body := postmarkFixture(t, "spf=pass; dkim=pass", nil)
	body = []byte(strings.ReplaceAll(string(body), "abc123def456@tasks.verevon.ai", "ABC123Def456@Tasks.Verevon.AI"))

	event, err := ParsePostmarkInboundPayload(body)
	if err != nil {
		t.Fatalf("ParsePostmarkInboundPayload() error = %v", err)
	}
	if event.Alias != "abc123def456" || event.ToDomain != "tasks.verevon.ai" {
		t.Fatalf("Alias/ToDomain = %q/%q, want lowercased", event.Alias, event.ToDomain)
	}
}
