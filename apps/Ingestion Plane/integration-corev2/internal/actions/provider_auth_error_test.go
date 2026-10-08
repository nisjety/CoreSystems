package actions

import (
	"context"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/triodelab/integration-corev2/internal/config"
	"github.com/triodelab/integration-corev2/internal/store"
)

// G1: a provider that refuses the stored credential or its scopes (401, or a
// 403 about permissions) is reported as a typed ProviderAuthError so the API
// can say "reconnect"; throttling and every other failure stay plain errors.

func rejectingServer(status int, headers map[string]string, body string) *httptest.Server {
	return httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		for key, value := range headers {
			w.Header().Set(key, value)
		}
		w.WriteHeader(status)
		_, _ = w.Write([]byte(body))
	}))
}

func executeMicrosoft(t *testing.T, server *httptest.Server, operation string, params map[string]any) error {
	t.Helper()
	service := NewService(config.Config{MicrosoftGraphBaseURL: server.URL}, server.Client())
	_, err := service.Execute(context.Background(), ExecuteInput{
		Connection: store.Connection{ProviderKey: "microsoft"}, AccessToken: "token",
		Operation: operation, Params: params, Body: map[string]any{"subject": "x", "bodyText": "y", "to": []string{"a@example.com"}},
	})
	return err
}

func TestProviderCredentialRejectionIsTyped(t *testing.T) {
	cases := []struct {
		name     string
		status   int
		headers  map[string]string
		body     string
		wantCode string
	}{
		{name: "graph 401", status: 401, body: `{"error":{"code":"InvalidAuthenticationToken","message":"secret detail that must not travel"}}`, wantCode: "InvalidAuthenticationToken"},
		{name: "graph 403", status: 403, body: `{"error":{"code":"ErrorAccessDenied","message":"x"}}`, wantCode: "ErrorAccessDenied"},
		{name: "403 with an empty body", status: 403, body: ``, wantCode: ""},
		{name: "401 with an html body", status: 401, body: `<html><body>Unauthorized</body></html>`, wantCode: ""},
		{name: "google 403 numeric code and reason", status: 403, body: `{"error":{"code":403,"errors":[{"reason":"insufficientPermissions"}]}}`, wantCode: "insufficientPermissions"},
		{name: "flat error string", status: 401, body: `{"error":"invalid_token"}`, wantCode: "invalid_token"},
		{name: "code is reduced to a safe charset and length", status: 403, body: `{"error":{"code":"bad code<script>` + strings.Repeat("x", 200) + `"}}`, wantCode: "badcodescript" + strings.Repeat("x", 51)},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			server := rejectingServer(tc.status, tc.headers, tc.body)
			defer server.Close()

			err := executeMicrosoft(t, server, "mail.messages", nil)
			rejection, ok := AsProviderAuthError(err)
			if !ok {
				t.Fatalf("error = %v, want a ProviderAuthError", err)
			}
			if rejection.Status != tc.status || rejection.Code != tc.wantCode {
				t.Fatalf("rejection = %+v, want status %d code %q", rejection, tc.status, tc.wantCode)
			}
			if strings.Contains(err.Error(), "secret detail") {
				t.Fatalf("the provider's message travelled in the error: %v", err)
			}
		})
	}
}

func TestProviderCredentialRejectionOnWriteAndContentReadIsTyped(t *testing.T) {
	server := rejectingServer(403, nil, `{"error":{"code":"ErrorAccessDenied"}}`)
	defer server.Close()

	if _, ok := AsProviderAuthError(executeMicrosoft(t, server, "mail.send", nil)); !ok {
		t.Fatal("a refused write (mail.send) was not typed")
	}
	if _, ok := AsProviderAuthError(executeMicrosoft(t, server, "drive.file.content", map[string]any{"itemId": "item-1"})); !ok {
		t.Fatal("a refused content read (drive.file.content) was not typed")
	}
}

func TestThrottlingAndOtherFailuresStayPlain(t *testing.T) {
	cases := []struct {
		name    string
		status  int
		headers map[string]string
		body    string
	}{
		{name: "403 with Retry-After", status: 403, headers: map[string]string{"Retry-After": "30"}, body: `{"error":{"code":"activityLimitReached"}}`},
		{name: "403 with exhausted rate limit header", status: 403, headers: map[string]string{"X-RateLimit-Remaining": "0"}, body: `{"message":"API rate limit exceeded"}`},
		{name: "google rateLimitExceeded", status: 403, body: `{"error":{"code":403,"errors":[{"reason":"rateLimitExceeded"}]}}`},
		{name: "google userRateLimitExceeded", status: 403, body: `{"error":{"errors":[{"reason":"userRateLimitExceeded"}]}}`},
		{name: "google dailyLimitExceeded", status: 403, body: `{"error":{"errors":[{"reason":"dailyLimitExceeded"}]}}`},
		{name: "google quota reason in details", status: 403, body: `{"error":{"details":[{"reason":"RATE_LIMIT_EXCEEDED"}]}}`},
		{name: "google RESOURCE_EXHAUSTED", status: 403, body: `{"error":{"status":"RESOURCE_EXHAUSTED"}}`},
		{name: "429", status: 429, body: `{"error":{"code":"TooManyRequests"}}`},
		{name: "500", status: 500, body: `{"error":{"code":"generalException"}}`},
		{name: "404", status: 404, body: `{"error":{"code":"itemNotFound"}}`},
		{name: "400", status: 400, body: `{"error":{"code":"invalidRequest"}}`},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			server := rejectingServer(tc.status, tc.headers, tc.body)
			defer server.Close()

			err := executeMicrosoft(t, server, "mail.messages", nil)
			if err == nil {
				t.Fatal("expected an error")
			}
			if _, typed := AsProviderAuthError(err); typed {
				t.Fatalf("error %v was typed as a credential rejection; reconnecting would not help", err)
			}
		})
	}
}

func TestSuccessfulResponsesAreUnaffected(t *testing.T) {
	server := rejectingServer(200, nil, `{"value":[]}`)
	defer server.Close()
	if err := executeMicrosoft(t, server, "mail.messages", nil); err != nil {
		t.Fatalf("Execute error: %v", err)
	}
}

func TestProviderWithoutReconnectFlowKeepsPlainFailure(t *testing.T) {
	// Okta's credential is server configuration, not a user connection, so a
	// refused call must not tell anyone to reconnect.
	server := rejectingServer(403, nil, `{"errorCode":"E0000006"}`)
	defer server.Close()
	service := NewService(config.Config{OktaAPIBaseURL: server.URL, OktaAPIToken: "okta-token"}, server.Client())
	_, err := service.Execute(context.Background(), ExecuteInput{
		Connection: store.Connection{ProviderKey: "okta"}, Operation: "users",
	})
	if err == nil {
		t.Fatal("expected an error")
	}
	if _, typed := AsProviderAuthError(err); typed {
		t.Fatalf("Okta failure %v was typed as reconnectable", err)
	}
	if !strings.Contains(err.Error(), "403") {
		t.Fatalf("error = %v, want the status kept in the plain failure", err)
	}
}

func TestOAuthProvidersOtherThanMicrosoftAlsoGetTheTypedRejection(t *testing.T) {
	server := rejectingServer(401, nil, `{"message":"Bad credentials"}`)
	defer server.Close()
	service := NewService(config.Config{GitHubAPIBaseURL: server.URL}, server.Client())
	_, err := service.Execute(context.Background(), ExecuteInput{
		Connection: store.Connection{ProviderKey: "github"}, AccessToken: "token", Operation: "user",
	})
	if _, typed := AsProviderAuthError(err); !typed {
		t.Fatalf("GitHub 401 error = %v, want a ProviderAuthError", err)
	}
}
