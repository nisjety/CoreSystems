package mailtask

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// authHeader is one entry of the Postmark inbound payload's Headers array.
type authHeader struct{ name, value string }

// authPayload builds a Postmark inbound payload (the shape documented at
// https://postmarkapp.com/developer/webhooks/inbound-webhook) addressed to the
// test alias, carrying exactly the given Headers array.
func authPayload(t *testing.T, headers []authHeader) []byte {
	t.Helper()
	list := make([]map[string]string, 0, len(headers))
	for _, header := range headers {
		list = append(list, map[string]string{"Name": header.name, "Value": header.value})
	}
	body, err := json.Marshal(map[string]any{
		"MessageID":         "provider-msg-auth",
		"From":              "owner@example.com",
		"FromFull":          map[string]string{"Email": "owner@example.com", "Name": "Task Owner"},
		"To":                "abc123def456@tasks.verevon.ai",
		"OriginalRecipient": "abc123def456@tasks.verevon.ai",
		"Subject":           "Fwd: please draft a reply",
		"TextBody":          "Body text",
		"Headers":           list,
	})
	if err != nil {
		t.Fatalf("marshal payload: %v", err)
	}
	return body
}

func readTestdata(t *testing.T, name string) []byte {
	t.Helper()
	body, err := os.ReadFile(filepath.Join("testdata", name))
	if err != nil {
		t.Fatalf("read testdata/%s: %v", name, err)
	}
	return body
}

// TestPostmarkDocumentedExamplePayloadsDerivePass pins the verdict on the two
// payload shapes Postmark itself documents. Their Headers arrays carry the
// SpamAssassin headers (X-Spam-Tests: DKIM_SIGNED,DKIM_VALID,DKIM_VALID_AU,
// SPF_PASS) and NO Authentication-Results header, which is why the receiver
// used to answer "none" for every real message.
func TestPostmarkDocumentedExamplePayloadsDerivePass(t *testing.T) {
	for _, name := range []string{"postmark_documented_full.json", "postmark_documented_minimal.json"} {
		t.Run(name, func(t *testing.T) {
			event, err := ParsePostmarkInboundPayload(readTestdata(t, name))
			if err != nil {
				t.Fatalf("ParsePostmarkInboundPayload() error = %v", err)
			}
			if event.AuthResult != "pass" {
				t.Fatalf("AuthResult = %q, want pass for Postmark's documented example headers", event.AuthResult)
			}
			if event.FromEmail != "owner@example.com" || event.Alias != "abc123def456" {
				t.Fatalf("FromEmail/Alias = %q/%q, fixture not parsed as expected", event.FromEmail, event.Alias)
			}
		})
	}
}

func TestPostmarkAuthVerdictFromHeaders(t *testing.T) {
	const documentedTests = "DKIM_SIGNED,DKIM_VALID,DKIM_VALID_AU,SPF_PASS"
	tests := []struct {
		name    string
		headers []authHeader
		want    string
	}{
		{
			name:    "documented SpamAssassin tests pass",
			headers: []authHeader{{"X-Spam-Status", "No"}, {"X-Spam-Tests", documentedTests}},
			want:    "pass",
		},
		{
			name:    "tokens survive case, spaces and folded lines",
			headers: []authHeader{{"X-Spam-Tests", "dkim_signed, dkim_valid,\r\n\tdkim_valid_au,\r\n spf_pass"}},
			want:    "pass",
		},
		{
			name:    "header name is matched case-insensitively",
			headers: []authHeader{{"x-spam-tests", documentedTests}},
			want:    "pass",
		},
		{
			name:    "extra unrelated tests do not matter",
			headers: []authHeader{{"X-Spam-Tests", "FREEMAIL_FROM,HTML_MESSAGE,RCVD_IN_DNSWL_LOW," + documentedTests}},
			want:    "pass",
		},
		{
			name: "forged second X-Spam-Tests next to the real one fails closed",
			headers: []authHeader{
				{"X-Spam-Tests", documentedTests},
				{"X-Spam-Tests", "DKIM_SIGNED,DKIM_VALID,DKIM_VALID_AU,SPF_PASS"},
			},
			want: "fail",
		},
		{
			name: "forged X-Spam-Tests claiming pass beside a real failing one fails",
			headers: []authHeader{
				{"X-Spam-Tests", "SPF_FAIL"},
				{"X-Spam-Tests", documentedTests},
			},
			want: "fail",
		},
		{
			name: "two Authentication-Results headers fail closed",
			headers: []authHeader{
				{"Authentication-Results", "forged; spf=pass; dkim=pass"},
				{"Authentication-Results", "mx.example.com; spf=pass; dkim=pass"},
			},
			want: "fail",
		},
		{
			name:    "DKIM_VALID without the author-aligned _AU fails",
			headers: []authHeader{{"X-Spam-Tests", "DKIM_SIGNED,DKIM_VALID,SPF_PASS"}},
			want:    "fail",
		},
		{
			name:    "aligned DKIM without SPF_PASS fails",
			headers: []authHeader{{"X-Spam-Tests", "DKIM_SIGNED,DKIM_VALID,DKIM_VALID_AU"}},
			want:    "fail",
		},
		{
			name:    "SPF_PASS without any DKIM token fails",
			headers: []authHeader{{"X-Spam-Tests", "SPF_PASS"}},
			want:    "fail",
		},
		{
			name:    "SPF softfail with aligned DKIM fails",
			headers: []authHeader{{"X-Spam-Tests", "DKIM_VALID,DKIM_VALID_AU,SPF_SOFTFAIL"}},
			want:    "fail",
		},
		{
			name:    "HELO-only SPF pass is not an envelope-from SPF pass",
			headers: []authHeader{{"X-Spam-Tests", "DKIM_VALID,DKIM_VALID_AU,SPF_HELO_PASS"}},
			want:    "fail",
		},
		{
			name:    "lookalike tokens are not matched by substring",
			headers: []authHeader{{"X-Spam-Tests", "MY_SPF_PASS,MY_DKIM_VALID_AU"}},
			want:    "none",
		},
		{
			name: "X-Spam-Tests without SPF/DKIM tokens does not fall through to a forgeable Authentication-Results",
			headers: []authHeader{
				{"X-Spam-Tests", "RCVD_IN_DNSWL_LOW"},
				{"Authentication-Results", "forged; spf=pass; dkim=pass"},
			},
			want: "none",
		},
		{
			name: "a failing X-Spam-Tests is not overridden by a forged Authentication-Results pass",
			headers: []authHeader{
				{"X-Spam-Tests", "SPF_FAIL,DKIM_SIGNED"},
				{"Authentication-Results", "forged; spf=pass; dkim=pass"},
			},
			want: "fail",
		},
		{
			name: "Authentication-Results is not consulted when X-Spam-Tests decides",
			headers: []authHeader{
				{"X-Spam-Tests", documentedTests},
				{"Authentication-Results", "hop-one; spf=pass; dkim=pass"},
				{"Authentication-Results", "hop-two; spf=pass; dkim=pass"},
			},
			want: "pass",
		},
		{
			name:    "Authentication-Results is the fallback when no X-Spam-Tests header exists",
			headers: []authHeader{{"Authentication-Results", "mx.example.com; spf=pass; dkim=pass"}},
			want:    "pass",
		},
		{
			name:    "Authentication-Results fallback still needs both SPF and DKIM",
			headers: []authHeader{{"Authentication-Results", "mx.example.com; spf=pass"}},
			want:    "fail",
		},
		{
			name:    "Received-SPF alone is not an authentication verdict",
			headers: []authHeader{{"Received-SPF", "Pass (sender SPF authorized) identity=mailfrom; client-ip=203.0.113.10"}},
			want:    "none",
		},
		{
			name:    "empty X-Spam-Tests value carries no verdict",
			headers: []authHeader{{"X-Spam-Tests", ""}},
			want:    "none",
		},
		{
			name:    "no relevant header at all",
			headers: []authHeader{{"X-Mailer", "Example Mailer 1.0"}},
			want:    "none",
		},
		{
			name:    "no headers",
			headers: nil,
			want:    "none",
		},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			event, err := ParsePostmarkInboundPayload(authPayload(t, test.headers))
			if err != nil {
				t.Fatalf("ParsePostmarkInboundPayload() error = %v", err)
			}
			if event.AuthResult != test.want {
				t.Fatalf("AuthResult = %q, want %q", event.AuthResult, test.want)
			}
		})
	}
}

// realRedactedFixture is the owner-supplied, redacted capture of one real
// Postmark inbound payload (owner checklist: send ONE self-forward test mail to
// the alias, copy the message's JSON from Postmark > Inbound > the message >
// JSON, redact it, save it here). Until it exists the receiver's rule is
// calibrated against Postmark's documentation only.
const realRedactedFixture = "postmark_real_redacted.json"

// TestPostmarkRealRedactedPayloadDerivesPass pins the verdict rule on a real
// capture. It is skipped, loudly, until the owner has supplied the fixture.
//
// Redaction rules for the fixture, so it can be committed: keep every header
// NAME, and keep the VALUES of X-Spam-Tests, X-Spam-Status, X-Spam-Score,
// X-Spam-Checker-Version, Received-SPF and any Authentication-Results header
// (they carry verdict tokens, no mail content); replace every other header
// value, the subject, both bodies, all attachment content, and every address
// with placeholders (From/FromFull must keep a syntactically valid address).
func TestPostmarkRealRedactedPayloadDerivesPass(t *testing.T) {
	body, err := os.ReadFile(filepath.Join("testdata", realRedactedFixture))
	if errors.Is(err, os.ErrNotExist) {
		t.Skipf("testdata/%s not present yet: the owner has not supplied a redacted real Postmark payload "+
			"(send one self-forward test mail, copy its JSON from Postmark, redact it, save it there). "+
			"Until then the sender-verification rule is pinned to Postmark's documentation only.", realRedactedFixture)
	}
	if err != nil {
		t.Fatalf("read testdata/%s: %v", realRedactedFixture, err)
	}
	event, err := ParsePostmarkInboundPayload(body)
	if err != nil {
		t.Fatalf("ParsePostmarkInboundPayload() error = %v", err)
	}
	if event.AuthResult != "pass" {
		t.Fatalf("AuthResult = %q for the real redacted payload, want pass. "+
			"The deriving rule does not match what Postmark really sends: fix the rule, not the fixture.", event.AuthResult)
	}
	if strings.TrimSpace(event.FromEmail) == "" {
		t.Fatal("real redacted payload has no usable From address")
	}
}
