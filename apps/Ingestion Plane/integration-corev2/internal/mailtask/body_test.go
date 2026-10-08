package mailtask

import (
	"context"
	"encoding/json"
	"strings"
	"sync"
	"testing"
	"time"
	"unicode/utf8"
)

// bodyTestResolver resolves the one test alias to an owner whose verified
// email matches authPayload's From.
type bodyTestResolver struct{}

func (bodyTestResolver) Resolve(_ context.Context, alias, _ string) (ResolvedAlias, bool, error) {
	if alias != "abc123def456" {
		return ResolvedAlias{}, false, nil
	}
	return ResolvedAlias{OrgID: "org-1", UserID: "user-1", VerifiedEmail: "owner@example.com"}, true, nil
}

// bodyTestGateway records what the pipeline forwards. It is safe for the
// concurrent deliveries the metrics test makes.
type bodyTestGateway struct {
	mu    sync.Mutex
	calls []GatewayIngestRequest
}

func (g *bodyTestGateway) Ingest(_ context.Context, req GatewayIngestRequest) error {
	g.mu.Lock()
	defer g.mu.Unlock()
	g.calls = append(g.calls, req)
	return nil
}

// forwardedBody runs one verified message with the given bodies through the
// whole handler and returns the body_text the gateway was sent.
func forwardedBody(t *testing.T, textBody, htmlBody string) string {
	t.Helper()
	payload, err := json.Marshal(map[string]any{
		"MessageID":         "provider-msg-body",
		"From":              "owner@example.com",
		"FromFull":          map[string]string{"Email": "owner@example.com", "Name": "Task Owner"},
		"To":                "abc123def456@tasks.verevon.ai",
		"OriginalRecipient": "abc123def456@tasks.verevon.ai",
		"Subject":           "Fwd: body handling",
		"TextBody":          textBody,
		"HtmlBody":          htmlBody,
		"Headers":           []map[string]string{{"Name": "X-Spam-Tests", "Value": "DKIM_SIGNED,DKIM_VALID,DKIM_VALID_AU,SPF_PASS"}},
	})
	if err != nil {
		t.Fatalf("marshal payload: %v", err)
	}
	gateway := &bodyTestGateway{}
	handler := &Handler{
		Provider:        "postmark",
		WebhookSecret:   "the-secret",
		ReceivingDomain: "tasks.verevon.ai",
		Resolver:        bodyTestResolver{},
		Gateway:         gateway,
		Replay:          NewReplayGuard(time.Hour),
		Now:             func() time.Time { return time.Date(2026, 10, 1, 12, 0, 0, 0, time.UTC) },
	}
	result := handler.HandleInboundPostmark(context.Background(), basicAuthHeader("postmark", "the-secret"), payload)
	if result.Reason != ReasonAccepted {
		t.Fatalf("Reason = %q, Err = %v, want %q", result.Reason, result.Err, ReasonAccepted)
	}
	if len(gateway.calls) != 1 {
		t.Fatalf("gateway calls = %d, want 1", len(gateway.calls))
	}
	return gateway.calls[0].BodyText
}

func TestForwardedBodyStripsHTMLTagsScriptsAndEntities(t *testing.T) {
	html := `<!DOCTYPE html><html><head><meta charset="utf-8"><title>Hidden title</title>` +
		`<style>p { color: red; } .x > .y { display:none }</style>` +
		`<script type="text/javascript">alert("secret-script-payload");</script></head>` +
		`<body><!-- a comment with <b>markup</b> --><p>Hello&nbsp;<b>Ola</b> &amp; Kari,</p>` +
		`<p>Please review the <a href="https://example.com/doc?a=1&amp;b=2" title="x > y">Q3 plan</a>.</p>` +
		`<div>Line one<br>Line two</div><ul><li>First</li><li>Second</li></ul>` +
		`<SCRIPT>document.write("upper-case-script")</SCRIPT>` +
		`<p>5 &lt; 6 &amp;&amp; 7 &gt; 3 &#169; &copy; &quot;quoted&quot;</p></body></html>`

	got := forwardedBody(t, "", html)

	for _, forbidden := range []string{
		"secret-script-payload", "upper-case-script", "Hidden title", "color: red", "a comment", "display:none", "alert(",
		"<p>", "<b>", "</", "href", "title=",
	} {
		if strings.Contains(got, forbidden) {
			t.Fatalf("body still contains %q:\n%s", forbidden, got)
		}
	}
	// The only angle brackets left are the ones the sender escaped as entities
	// ("5 &lt; 6 ... 7 &gt; 3"), decoded to plain text.
	if strings.Count(got, "<") != 1 || strings.Count(got, ">") != 1 {
		t.Fatalf("unexpected angle brackets in %q", got)
	}
	for _, want := range []string{
		"Hello Ola & Kari,", "Please review the Q3 plan.", "Line one\nLine two", "- First", "- Second", `5 < 6 && 7 > 3 © © "quoted"`,
	} {
		if !strings.Contains(got, want) {
			t.Fatalf("body missing %q:\n%s", want, got)
		}
	}
	if strings.Contains(got, "\u00a0") {
		t.Fatalf("non-breaking spaces were not normalised: %q", got)
	}
	if strings.Contains(got, "\n\n\n") {
		t.Fatalf("blank lines were not collapsed: %q", got)
	}
}

func TestForwardedBodyPrefersAPlainTextBodyAndLeavesItUntouched(t *testing.T) {
	got := forwardedBody(t, "Plain  text <not a tag> &amp; stays as sent.", "<p>HTML version</p>")
	if got != "Plain  text <not a tag> &amp; stays as sent." {
		t.Fatalf("body = %q, want the TextBody verbatim", got)
	}
}

func TestForwardedBodyUsesHTMLWhenTextBodyIsBlank(t *testing.T) {
	got := forwardedBody(t, "  \n ", "<p>Only html here</p>")
	if got != "Only html here" {
		t.Fatalf("body = %q", got)
	}
}

func TestForwardedBodyDropsAnUnterminatedScript(t *testing.T) {
	got := forwardedBody(t, "", "<p>Before</p><script>var leaked = 'after-unterminated-script';")
	if strings.Contains(got, "leaked") || strings.Contains(got, "after-unterminated-script") {
		t.Fatalf("unterminated script content survived: %q", got)
	}
	if got != "Before" {
		t.Fatalf("body = %q, want %q", got, "Before")
	}
}

func TestForwardedBodyIsCappedWithATruncationMarker(t *testing.T) {
	long := strings.Repeat("abcdefghij", 7000) // 70,000 characters
	for name, run := range map[string]func(*testing.T) string{
		"text body": func(t *testing.T) string { return forwardedBody(t, long, "") },
		"html body": func(t *testing.T) string { return forwardedBody(t, "", "<p>"+long+"</p>") },
	} {
		t.Run(name, func(t *testing.T) {
			got := run(t)
			if n := utf8.RuneCountInString(got); n > 60000 {
				t.Fatalf("body has %d characters, want at most 60000", n)
			}
			if !strings.HasSuffix(got, "[Message body truncated: only the first 60,000 characters were kept.]") {
				t.Fatalf("missing truncation marker, tail = %q", got[len(got)-90:])
			}
			if !strings.HasPrefix(got, "abcdefghij") {
				t.Fatalf("body does not start with the original text: %q", got[:30])
			}
		})
	}
}

func TestForwardedBodyUnderTheCapIsNotMarked(t *testing.T) {
	exact := strings.Repeat("x", 60000)
	got := forwardedBody(t, exact, "")
	if got != exact {
		t.Fatalf("a body of exactly 60000 characters must pass through unchanged (len %d)", len(got))
	}
}

func TestForwardedBodyCapCountsCharactersNotBytes(t *testing.T) {
	multibyte := strings.Repeat("ø", 59000) // 118,000 bytes, 59,000 characters
	got := forwardedBody(t, multibyte, "")
	if got != multibyte {
		t.Fatalf("59,000 multibyte characters are under the cap and must not be truncated (got %d runes)", utf8.RuneCountInString(got))
	}
	over := strings.Repeat("ø", 61000)
	got = forwardedBody(t, over, "")
	if n := utf8.RuneCountInString(got); n > 60000 || !utf8.ValidString(got) {
		t.Fatalf("over-cap multibyte body: %d runes, valid=%v", n, utf8.ValidString(got))
	}
	if !strings.Contains(got, "truncated") {
		t.Fatal("over-cap multibyte body carries no truncation marker")
	}
}
