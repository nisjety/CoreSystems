package api

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"io"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/rs/zerolog"
	"github.com/triodelab/integration-corev2/internal/mailtask"
)

type mailTaskTestResolver struct{}

func (mailTaskTestResolver) Resolve(_ context.Context, alias, _ string) (mailtask.ResolvedAlias, bool, error) {
	if alias != "abc123def456" {
		return mailtask.ResolvedAlias{}, false, nil
	}
	return mailtask.ResolvedAlias{OrgID: "org-1", UserID: "user-1", VerifiedEmail: "owner@example.com"}, true, nil
}

type mailTaskTestGateway struct{}

func (mailTaskTestGateway) Ingest(context.Context, mailtask.GatewayIngestRequest) error { return nil }

// TestMailTaskWebhookLogsAContentFreeDiagnosticForADroppedSenderAndExposesCounters
// proves the mass-drop signal reaches the operator through the real route: a
// message dropped for sender verification still answers 200 (never leak why to
// the sender), but logs one warn line carrying the failing check, the derived
// verdict and the header NAMES -- no value, subject or address -- and moves the
// counters served on /metrics.
func TestMailTaskWebhookLogsAContentFreeDiagnosticForADroppedSenderAndExposesCounters(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	var logs bytes.Buffer
	logger := zerolog.New(&logs)
	handler := &mailtask.Handler{
		Provider:        "postmark",
		WebhookSecret:   "the-secret",
		ReceivingDomain: "tasks.verevon.ai",
		Resolver:        mailTaskTestResolver{},
		Gateway:         mailTaskTestGateway{},
		Replay:          mailtask.NewReplayGuard(time.Hour),
	}
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service, MailTask: handler, Logger: &logger})

	payload, err := json.Marshal(map[string]any{
		"MessageID":         "provider-msg-sentinel",
		"From":              "owner@example.com",
		"FromFull":          map[string]string{"Email": "owner@example.com", "Name": "SENTINEL-NAME"},
		"To":                "abc123def456@tasks.verevon.ai",
		"OriginalRecipient": "abc123def456@tasks.verevon.ai",
		"Subject":           "SENTINEL-SUBJECT",
		"TextBody":          "SENTINEL-BODY",
		"Headers": []map[string]string{
			{"Name": "X-Mailer", "Value": "SENTINEL-HEADER-VALUE"},
			{"Name": "Received-SPF", "Value": "Pass SENTINEL-HEADER-VALUE"},
		},
	})
	if err != nil {
		t.Fatalf("marshal payload: %v", err)
	}
	request := httptest.NewRequest("POST", "/api/v1/webhooks/mail-task/postmark", bytes.NewReader(payload))
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("Authorization", "Basic "+base64.StdEncoding.EncodeToString([]byte("postmark:the-secret")))
	response, err := app.Test(request)
	if err != nil {
		t.Fatalf("webhook request error: %v", err)
	}
	if response.StatusCode != 200 {
		t.Fatalf("webhook status = %d, want 200 (a dropped sender is never revealed to the sender)", response.StatusCode)
	}

	logged := logs.String()
	var diagnosticLine string
	for _, line := range strings.Split(logged, "\n") {
		if strings.Contains(line, "mail-task inbound webhook") {
			diagnosticLine = line
		}
	}
	if diagnosticLine == "" {
		t.Fatalf("no mail-task log line in:\n%s", logged)
	}
	for _, want := range []string{
		`"level":"warn"`, `"reason":"sender_not_verified"`, `"sender_diagnostic":"`,
		"cause=auth_not_pass", "verdict=none", "rule=none", "detail=no_usable_header", "headers=received-spf,x-mailer",
	} {
		if !strings.Contains(diagnosticLine, want) {
			t.Fatalf("log line %s is missing %q", diagnosticLine, want)
		}
	}
	for _, sentinel := range []string{"SENTINEL", "owner@example.com", "abc123def456"} {
		if strings.Contains(logged, sentinel) {
			t.Fatalf("logs leak %q:\n%s", sentinel, logged)
		}
	}

	metricsRequest := httptest.NewRequest("GET", "/metrics", nil)
	metricsRequest.Header.Set("X-Internal-API-Key", "dev-key")
	metricsResponse, err := app.Test(metricsRequest)
	if err != nil {
		t.Fatalf("metrics request error: %v", err)
	}
	metricsBody, err := io.ReadAll(metricsResponse.Body)
	if err != nil {
		t.Fatalf("read metrics: %v", err)
	}
	metrics := string(metricsBody)
	for _, want := range []string{
		`integration_mailtask_inbound_total{reason="sender_not_verified"} 1`,
		`integration_mailtask_sender_check_total{outcome="dropped",cause="auth_not_pass",verdict="none",rule="none",detail="no_usable_header"} 1`,
	} {
		if !strings.Contains(metrics, want) {
			t.Fatalf("metrics are missing %q:\n%s", want, metrics)
		}
	}
	if strings.Contains(metrics, "SENTINEL") {
		t.Fatalf("metrics leak a sentinel:\n%s", metrics)
	}
}
