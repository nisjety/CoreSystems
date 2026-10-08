package mailtask

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"
	"sync"
	"testing"
	"time"
)

const spamTestsPass = "DKIM_SIGNED,DKIM_VALID,DKIM_VALID_AU,SPF_PASS"

// diagPayload builds a payload from a base message, applying overrides on top
// (a nil override value removes the key).
func diagPayload(t *testing.T, headers []authHeader, overrides map[string]any) []byte {
	t.Helper()
	list := make([]map[string]string, 0, len(headers))
	for _, header := range headers {
		list = append(list, map[string]string{"Name": header.name, "Value": header.value})
	}
	payload := map[string]any{
		"MessageID":         "provider-msg-diag",
		"From":              "owner@example.com",
		"FromFull":          map[string]string{"Email": "owner@example.com", "Name": "Task Owner"},
		"To":                "abc123def456@tasks.verevon.ai",
		"OriginalRecipient": "abc123def456@tasks.verevon.ai",
		"Subject":           "Fwd: diagnostics",
		"TextBody":          "Body text",
		"Headers":           list,
	}
	for key, value := range overrides {
		if value == nil {
			delete(payload, key)
			continue
		}
		payload[key] = value
	}
	body, err := json.Marshal(payload)
	if err != nil {
		t.Fatalf("marshal payload: %v", err)
	}
	return body
}

func newDiagHandler() (*Handler, *bodyTestGateway) {
	gateway := &bodyTestGateway{}
	return &Handler{
		Provider:        "postmark",
		WebhookSecret:   "the-secret",
		ReceivingDomain: "tasks.verevon.ai",
		Resolver:        bodyTestResolver{},
		Gateway:         gateway,
		Replay:          NewReplayGuard(time.Hour),
		Now:             func() time.Time { return time.Date(2026, 10, 1, 12, 0, 0, 0, time.UTC) },
	}, gateway
}

func deliver(h *Handler, body []byte) Result {
	return h.HandleInboundPostmark(context.Background(), basicAuthHeader("postmark", "the-secret"), body)
}

func TestAuthDiagnosticNamesTheDecidingRuleAndWhy(t *testing.T) {
	tests := []struct {
		name    string
		headers []authHeader
		verdict string
		rule    string
		detail  string
	}{
		{"documented tests pass", []authHeader{{"X-Spam-Tests", spamTestsPass}}, "pass", "x-spam-tests", "ok"},
		{"no scan headers", []authHeader{{"X-Mailer", "m"}}, "none", "none", "no_usable_header"},
		{"duplicate X-Spam-Tests", []authHeader{{"X-Spam-Tests", spamTestsPass}, {"X-Spam-Tests", spamTestsPass}}, "fail", "x-spam-tests", "duplicate_header"},
		{"DKIM_VALID without _AU", []authHeader{{"X-Spam-Tests", "DKIM_VALID,SPF_PASS"}}, "fail", "x-spam-tests", "dkim_not_aligned"},
		{"SPF not pass", []authHeader{{"X-Spam-Tests", "DKIM_VALID_AU,SPF_SOFTFAIL"}}, "fail", "x-spam-tests", "spf_not_pass"},
		{"neither SPF nor DKIM pass", []authHeader{{"X-Spam-Tests", "SPF_FAIL,DKIM_INVALID"}}, "fail", "x-spam-tests", "spf_and_dkim_not_pass"},
		{"no SPF/DKIM tokens", []authHeader{{"X-Spam-Tests", "RCVD_IN_DNSWL_LOW"}}, "none", "x-spam-tests", "no_spf_or_dkim_tokens"},
		{"Authentication-Results fallback", []authHeader{{"Authentication-Results", "mx; spf=pass; dkim=pass"}}, "pass", "authentication-results", "ok"},
		{"Authentication-Results failing", []authHeader{{"Authentication-Results", "mx; spf=fail; dkim=pass"}}, "fail", "authentication-results", "spf_or_dkim_not_pass"},
		{"duplicate Authentication-Results", []authHeader{{"Authentication-Results", "a; spf=pass; dkim=pass"}, {"Authentication-Results", "b; spf=pass; dkim=pass"}}, "fail", "authentication-results", "duplicate_header"},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			event, err := ParsePostmarkInboundPayload(authPayload(t, test.headers))
			if err != nil {
				t.Fatalf("ParsePostmarkInboundPayload() error = %v", err)
			}
			got := event.AuthDiagnostic
			if got.Verdict != test.verdict || got.Rule != test.rule || got.Detail != test.detail {
				t.Fatalf("diagnostic = %s/%s/%s, want %s/%s/%s", got.Verdict, got.Rule, got.Detail, test.verdict, test.rule, test.detail)
			}
			if event.AuthResult != got.Verdict {
				t.Fatalf("AuthResult %q disagrees with the diagnostic verdict %q", event.AuthResult, got.Verdict)
			}
		})
	}
}

// TestDroppedMessageDiagnosticCarriesNoHeaderValuesOrContent is the log-line
// test: it plants a distinctive sentinel in every place mail content can live
// and asserts none of them reaches the diagnostic line or the counters, while
// the header NAMES and the derived verdict do.
func TestDroppedMessageDiagnosticCarriesNoHeaderValuesOrContent(t *testing.T) {
	sentinels := []string{
		"SENTINEL-HEADER-VALUE", "SENTINEL-SUBJECT", "SENTINEL-BODY", "SENTINEL-HTML",
		"attacker@evil.example", "SENTINEL-NAME", "SENTINEL-ATTACHMENT", "SENTINEL-TOKEN",
		"owner@example.com", "abc123def456", "tasks.verevon.ai", "provider-msg-diag",
	}
	headers := []authHeader{
		{"X-Mailer", "SENTINEL-HEADER-VALUE mailer"},
		{"X-Spam-Score", "SENTINEL-HEADER-VALUE 9.9"},
		{"Received-SPF", "None (SENTINEL-HEADER-VALUE) envelope-from=attacker@evil.example"},
		{"DKIM-Signature", "v=1; d=evil.example; b=SENTINEL-HEADER-VALUE"},
		{"Authentication-Results", "forged SENTINEL-HEADER-VALUE; spf=fail"},
		{"X-Secret-attacker@evil.example", "SENTINEL-HEADER-VALUE"}, // a name carrying an address must not be logged
		{"X-Has Space SENTINEL-NAME", "SENTINEL-HEADER-VALUE"},      // a name outside the alphabet must not be logged
		{"X-" + strings.Repeat("a", 80), "SENTINEL-HEADER-VALUE"},   // an over-long name must not be logged
		{"Subject", "SENTINEL-SUBJECT"},
	}
	body := diagPayload(t, headers, map[string]any{
		"From":        "attacker@evil.example",
		"FromFull":    map[string]string{"Email": "attacker@evil.example", "Name": "SENTINEL-NAME"},
		"Subject":     "SENTINEL-SUBJECT",
		"TextBody":    "SENTINEL-BODY",
		"HtmlBody":    "<p>SENTINEL-HTML</p>",
		"Attachments": []map[string]string{{"Name": "SENTINEL-ATTACHMENT.pdf", "Content": "U0VOVElORUw=", "ContentType": "application/pdf"}},
	})
	h, gateway := newDiagHandler()

	result := deliver(h, body)

	if result.Reason != ReasonSenderNotVerified {
		t.Fatalf("Reason = %q, want %q", result.Reason, ReasonSenderNotVerified)
	}
	if len(gateway.calls) != 0 {
		t.Fatal("a dropped message must not reach the gateway")
	}
	line := result.Diagnostic
	for _, want := range []string{
		"cause=auth_not_pass", "verdict=fail", "rule=authentication-results", "detail=spf_or_dkim_not_pass",
		"header_count=6", "headers=authentication-results,dkim-signature,received-spf,subject,x-mailer,x-spam-score",
	} {
		if !strings.Contains(line, want) {
			t.Fatalf("diagnostic %q is missing %q", line, want)
		}
	}
	if strings.ContainsAny(line, "\r\n") {
		t.Fatalf("diagnostic must be a single line: %q", line)
	}
	metrics := h.RenderMetrics()
	for _, want := range []string{
		`integration_mailtask_inbound_total{reason="sender_not_verified"} 1`,
		`integration_mailtask_sender_check_total{outcome="dropped",cause="auth_not_pass",verdict="fail",rule="authentication-results",detail="spf_or_dkim_not_pass"} 1`,
	} {
		if !strings.Contains(metrics, want) {
			t.Fatalf("metrics are missing %q:\n%s", want, metrics)
		}
	}
	for _, sentinel := range sentinels {
		if strings.Contains(strings.ToLower(line), strings.ToLower(sentinel)) {
			t.Fatalf("diagnostic leaks %q: %s", sentinel, line)
		}
		if strings.Contains(strings.ToLower(metrics), strings.ToLower(sentinel)) {
			t.Fatalf("metrics leak %q:\n%s", sentinel, metrics)
		}
	}
}

func TestDroppedMessageDiagnosticDistinguishesTheFailingCheck(t *testing.T) {
	good := []authHeader{{"X-Spam-Tests", spamTestsPass}}
	tests := []struct {
		name      string
		headers   []authHeader
		overrides map[string]any
		want      []string
	}{
		{
			name:    "real mail with an unrecognised header set",
			headers: []authHeader{{"X-Mailer", "m"}},
			want:    []string{"cause=auth_not_pass", "verdict=none", "rule=none", "detail=no_usable_header", "headers=x-mailer"},
		},
		{
			name:    "authenticated mail from someone other than the owner",
			headers: good,
			overrides: map[string]any{
				"From":     "someone.else@example.com",
				"FromFull": map[string]string{"Email": "someone.else@example.com"},
			},
			want: []string{"cause=from_mismatch", "verdict=pass", "rule=x-spam-tests", "detail=ok"},
		},
		{
			name:    "forged duplicate scan header",
			headers: []authHeader{{"X-Spam-Tests", spamTestsPass}, {"x-spam-tests", spamTestsPass}},
			want:    []string{"cause=auth_not_pass", "verdict=fail", "rule=x-spam-tests", "detail=duplicate_header"},
		},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			h, _ := newDiagHandler()
			result := deliver(h, diagPayload(t, test.headers, test.overrides))
			if result.Reason != ReasonSenderNotVerified {
				t.Fatalf("Reason = %q, want %q", result.Reason, ReasonSenderNotVerified)
			}
			for _, want := range test.want {
				if !strings.Contains(result.Diagnostic, want) {
					t.Fatalf("diagnostic %q is missing %q", result.Diagnostic, want)
				}
			}
		})
	}
}

func TestAcceptedMessageHasNoDiagnosticAndIsCounted(t *testing.T) {
	h, gateway := newDiagHandler()

	result := deliver(h, diagPayload(t, []authHeader{{"X-Spam-Tests", spamTestsPass}}, nil))

	if result.Reason != ReasonAccepted || result.Diagnostic != "" {
		t.Fatalf("Reason = %q, Diagnostic = %q, want accepted with no diagnostic", result.Reason, result.Diagnostic)
	}
	if len(gateway.calls) != 1 {
		t.Fatalf("gateway calls = %d, want 1", len(gateway.calls))
	}
	metrics := h.RenderMetrics()
	for _, want := range []string{
		`integration_mailtask_inbound_total{reason="accepted"} 1`,
		`integration_mailtask_sender_check_total{outcome="verified",cause="none",verdict="pass",rule="x-spam-tests",detail="ok"} 1`,
	} {
		if !strings.Contains(metrics, want) {
			t.Fatalf("metrics are missing %q:\n%s", want, metrics)
		}
	}
}

func TestMetricsCountEveryInboundOutcomeAndSurviveARepeat(t *testing.T) {
	h, _ := newDiagHandler()
	if got := h.RenderMetrics(); got != "" {
		t.Fatalf("metrics before any request = %q, want empty", got)
	}
	if got := (*Handler)(nil).RenderMetrics(); got != "" {
		t.Fatalf("nil handler metrics = %q, want empty", got)
	}

	for i := 0; i < 3; i++ {
		deliver(h, diagPayload(t, []authHeader{{"X-Mailer", "m"}}, map[string]any{"MessageID": fmt.Sprintf("drop-%d", i)}))
	}
	deliver(h, diagPayload(t, []authHeader{{"X-Spam-Tests", spamTestsPass}}, map[string]any{"MessageID": "ok-1"}))
	h.HandleInboundPostmark(context.Background(), "", diagPayload(t, nil, nil))

	metrics := h.RenderMetrics()
	for _, want := range []string{
		`integration_mailtask_inbound_total{reason="sender_not_verified"} 3`,
		`integration_mailtask_inbound_total{reason="accepted"} 1`,
		`integration_mailtask_inbound_total{reason="unauthorized"} 1`,
		`integration_mailtask_sender_check_total{outcome="dropped",cause="auth_not_pass",verdict="none",rule="none",detail="no_usable_header"} 3`,
		"# TYPE integration_mailtask_inbound_total counter",
		"# TYPE integration_mailtask_sender_check_total counter",
	} {
		if !strings.Contains(metrics, want) {
			t.Fatalf("metrics are missing %q:\n%s", want, metrics)
		}
	}
	if metrics != h.RenderMetrics() {
		t.Fatal("rendering metrics twice must give identical output")
	}
}

func TestMetricsAreSafeUnderConcurrentDeliveries(t *testing.T) {
	h, _ := newDiagHandler()
	var wg sync.WaitGroup
	for i := 0; i < 40; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			headers := []authHeader{{"X-Spam-Tests", spamTestsPass}}
			if i%2 == 0 {
				headers = []authHeader{{"X-Mailer", "m"}}
			}
			deliver(h, diagPayload(t, headers, map[string]any{"MessageID": fmt.Sprintf("concurrent-%d", i)}))
			_ = h.RenderMetrics()
		}(i)
	}
	wg.Wait()
	metrics := h.RenderMetrics()
	for _, want := range []string{
		`integration_mailtask_inbound_total{reason="accepted"} 20`,
		`integration_mailtask_inbound_total{reason="sender_not_verified"} 20`,
	} {
		if !strings.Contains(metrics, want) {
			t.Fatalf("metrics are missing %q:\n%s", want, metrics)
		}
	}
}

func TestSanitizedHeaderNamesAreBoundedSortedAndRestrictedToTheAlphabet(t *testing.T) {
	counts := map[string]int{
		"x-b": 1, "x-a": 2, "": 1, "has space": 1, "with@at": 1, "upper-Case": 1, "ok-9": 1,
		strings.Repeat("n", maxDiagHeaderNameLength):   1,
		strings.Repeat("n", maxDiagHeaderNameLength+1): 1,
		"x-\u00e9": 1,
	}
	got := sanitizedHeaderNames(counts)
	want := []string{strings.Repeat("n", maxDiagHeaderNameLength), "ok-9", "x-a", "x-b"}
	if strings.Join(got, "|") != strings.Join(want, "|") {
		t.Fatalf("names = %q, want %q", got, want)
	}

	many := make(map[string]int)
	for i := 0; i < maxDiagHeaderNames+30; i++ {
		many[fmt.Sprintf("x-h%03d", i)] = 1
	}
	if got := sanitizedHeaderNames(many); len(got) != maxDiagHeaderNames {
		t.Fatalf("len(names) = %d, want it capped at %d", len(got), maxDiagHeaderNames)
	}
}

func TestMetricLabelValueFlattensAnythingOutsideTheSafeAlphabet(t *testing.T) {
	if got := metricLabelValue("a\"b\\c\nd e"); got != "a_b_c_d_e" {
		t.Fatalf("label = %q", got)
	}
	if got := metricLabelValue(""); got != "unspecified" {
		t.Fatalf("empty label = %q", got)
	}
}

// TestSenderRejectionCauseAgreesWithVerifySender keeps the two views of the one
// anti-spoofing decision from drifting: VerifySender is true exactly when there
// is no rejection cause, and the cause follows the order of the checks.
func TestSenderRejectionCauseAgreesWithVerifySender(t *testing.T) {
	tests := []struct {
		name       string
		authResult string
		from       string
		verified   string
		want       string
	}{
		{"verified", "pass", "owner@example.com", "owner@example.com", ""},
		{"verified, case-insensitive", "pass", "Owner@Example.com", "owner@example.com", ""},
		{"auth fail", "fail", "owner@example.com", "owner@example.com", senderCauseAuthNotPass},
		{"auth none", "none", "owner@example.com", "owner@example.com", senderCauseAuthNotPass},
		{"auth not pass wins over a mismatch", "none", "attacker@example.com", "owner@example.com", senderCauseAuthNotPass},
		{"no verified email on file", "pass", "owner@example.com", "  ", senderCauseNoVerifiedEmail},
		{"different sender", "pass", "attacker@example.com", "owner@example.com", senderCauseFromMismatch},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			event := InboundMailTaskEvent{AuthResult: test.authResult, FromEmail: test.from}
			if got := senderRejectionCause(event, test.verified); got != test.want {
				t.Fatalf("senderRejectionCause() = %q, want %q", got, test.want)
			}
			if got := VerifySender(event, test.verified); got != (test.want == "") {
				t.Fatalf("VerifySender() = %v disagrees with cause %q", got, test.want)
			}
		})
	}
}
