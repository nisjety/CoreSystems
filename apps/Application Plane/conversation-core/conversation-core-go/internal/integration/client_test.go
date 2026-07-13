package integration

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"
)

func TestSendUsesTenantBoundServiceBearerAndCachesPerOrganization(t *testing.T) {
	now := time.Date(2026, 7, 13, 12, 0, 0, 0, time.UTC)
	tokenCalls := map[string]int{}
	authServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodPost || r.URL.Path != "/api/ingestion/internal-token" {
			t.Errorf("auth request = %s %s", r.Method, r.URL.Path)
		}
		if r.Header.Get("x-service-id") != "conversation-core" || r.Header.Get("x-service-api-key") != "service-credential-at-least-32-bytes" {
			t.Errorf("service principal headers missing")
		}
		var body struct {
			OrgID  string   `json:"orgId"`
			Scopes []string `json:"scopes"`
			Reason string   `json:"reason"`
		}
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Fatalf("decode token request: %v", err)
		}
		if body.OrgID == "" || body.Reason == "" || fmt.Sprint(body.Scopes) != "[integration:read integration:write]" {
			t.Errorf("token request = %#v", body)
		}
		tokenCalls[body.OrgID]++
		_ = json.NewEncoder(w).Encode(map[string]any{
			"token":     "token-for-" + body.OrgID,
			"expiresAt": now.Add(5 * time.Minute).Format(time.RFC3339),
		})
	}))
	defer authServer.Close()

	actionCalls := 0
	integrationServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		actionCalls++
		wantToken := "Bearer token-for-org-1"
		if actionCalls == 3 {
			wantToken = "Bearer token-for-org-2"
		}
		if got := r.Header.Get("Authorization"); got != wantToken {
			t.Errorf("Authorization = %q, want %q", got, wantToken)
		}
		for _, legacy := range []string{"x-internal-api-key", "x-org-id", "x-user-id"} {
			if got := r.Header.Get(legacy); got != "" {
				t.Errorf("tenant action leaked legacy header %s", legacy)
			}
		}
		if r.Header.Get("x-service-id") != "" || r.Header.Get("x-service-api-key") != "" {
			t.Error("durable Auth Core service credential leaked to integration-corev2")
		}
		_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"result":{"ts":"42.1"}}}}`))
	}))
	defer integrationServer.Close()

	client := NewClient(integrationServer.URL, "internal-only-key",
		WithServicePrincipal(authServer.URL, "conversation-core", "service-credential-at-least-32-bytes"),
		WithClock(func() time.Time { return now }),
		WithWriteAttestor(&recordingAttestor{compact: "test-proof"}),
		WithHTTPClient(&http.Client{Timeout: time.Second}),
	)
	for _, orgID := range []string{"org-1", "org-1", "org-2"} {
		_, err := client.Send(t.Context(), authorizedTestRequest(t, SendRequest{
			OrgID: orgID, ActorUserID: "user-forged", Provider: "slack", ConnectionID: "conn-1",
			ProviderThreadID: "C123", BodyText: "hello", ApprovalID: "approval-1", IdempotencyKey: "conversation:approval-1",
		}))
		if err != nil {
			t.Fatalf("Send(%s): %v", orgID, err)
		}
	}
	if tokenCalls["org-1"] != 1 || tokenCalls["org-2"] != 1 {
		t.Fatalf("token calls = %#v, want one cached token per org", tokenCalls)
	}
}

func TestFetchActiveConnectionIDUsesTenantBoundBearerWithoutLegacyHeaders(t *testing.T) {
	authCalls := 0
	authServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		authCalls++
		var request serviceTokenRequest
		if err := json.NewDecoder(r.Body).Decode(&request); err != nil || request.OrgID != "org-1" {
			t.Fatalf("token request = %#v err=%v", request, err)
		}
		_ = json.NewEncoder(w).Encode(serviceTokenResponse{
			Token:     "org-1-read-token",
			ExpiresAt: time.Now().UTC().Add(5 * time.Minute).Format(time.RFC3339),
		})
	}))
	defer authServer.Close()
	integrationServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Authorization") != "Bearer org-1-read-token" {
			t.Errorf("Authorization = %q", r.Header.Get("Authorization"))
		}
		for _, forbidden := range []string{"x-internal-api-key", "x-org-id", "x-user-id", "x-service-api-key"} {
			if r.Header.Get(forbidden) != "" {
				t.Errorf("tenant read leaked %s", forbidden)
			}
		}
		_, _ = w.Write([]byte(`{"data":{"connections":[{"id":"conn-1","status":"active"}]}}`))
	}))
	defer integrationServer.Close()

	client := NewClient(integrationServer.URL, "internal-webhook-key",
		WithServicePrincipal(authServer.URL, "conversation-core", "service-credential-at-least-32-bytes"),
	)
	connectionID, err := client.FetchActiveConnectionID(t.Context(), "org-1", "whatsapp")
	if err != nil || connectionID != "conn-1" {
		t.Fatalf("FetchActiveConnectionID = %q, %v", connectionID, err)
	}
	if authCalls != 1 {
		t.Fatalf("auth calls = %d, want one", authCalls)
	}
}

func TestFetchActiveConnectionIDSkipsEmptyAndInactiveCandidates(t *testing.T) {
	requestedProviders := []string{}
	integrationServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		provider := r.URL.Query().Get("providerKey")
		requestedProviders = append(requestedProviders, provider)
		status := "inactive"
		connectionID := "old-connection"
		if provider == "meta" {
			status = "active"
			connectionID = "active-connection"
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"data": map[string]any{"connections": []map[string]string{{"id": connectionID, "status": status}}}})
	}))
	defer integrationServer.Close()

	client := newTestClient(t, integrationServer.URL)
	connectionID, err := client.FetchActiveConnectionID(t.Context(), "org-1", "", "whatsapp", "meta")
	if err != nil || connectionID != "active-connection" {
		t.Fatalf("FetchActiveConnectionID = %q, %v", connectionID, err)
	}
	if fmt.Sprint(requestedProviders) != "[whatsapp meta]" {
		t.Fatalf("requested providers = %v", requestedProviders)
	}
}

func TestFetchActiveConnectionIDBoundsAndRedactsFailureResponses(t *testing.T) {
	for _, testCase := range []struct {
		name   string
		status int
		body   string
	}{
		{name: "upstream error", status: http.StatusBadGateway, body: `provider-sensitive-detail`},
		{name: "oversized", status: http.StatusOK, body: strings.Repeat("x", actionResponseMaxBytes+1)},
		{name: "malformed", status: http.StatusOK, body: `{`},
	} {
		t.Run(testCase.name, func(t *testing.T) {
			integrationServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
				w.WriteHeader(testCase.status)
				_, _ = w.Write([]byte(testCase.body))
			}))
			defer integrationServer.Close()

			_, err := newTestClient(t, integrationServer.URL).FetchActiveConnectionID(t.Context(), "org-1", "meta")
			if err == nil {
				t.Fatal("FetchActiveConnectionID error = nil")
			}
			if strings.Contains(err.Error(), "provider-sensitive-detail") {
				t.Fatalf("error leaked upstream body: %v", err)
			}
		})
	}
}

func TestSendRefreshesExpiringServiceBearer(t *testing.T) {
	now := time.Date(2026, 7, 13, 12, 0, 0, 0, time.UTC)
	tokenCalls := 0
	authServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		tokenCalls++
		_ = json.NewEncoder(w).Encode(map[string]any{
			"token":     fmt.Sprintf("token-%d", tokenCalls),
			"expiresAt": now.Add(5 * time.Minute).Format(time.RFC3339),
		})
	}))
	defer authServer.Close()
	integrationServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"result":{"ts":"42.1"}}}}`))
	}))
	defer integrationServer.Close()

	client := NewClient(integrationServer.URL, "internal-only-key",
		WithServicePrincipal(authServer.URL, "conversation-core", "service-credential-at-least-32-bytes"),
		WithClock(func() time.Time { return now }),
		WithWriteAttestor(&recordingAttestor{compact: "test-proof"}),
	)
	request := authorizedTestRequest(t, SendRequest{OrgID: "org-1", Provider: "slack", ConnectionID: "conn-1", ProviderThreadID: "C123", BodyText: "hello"})
	if _, err := client.Send(t.Context(), request); err != nil {
		t.Fatalf("first Send: %v", err)
	}
	now = now.Add(4*time.Minute + 31*time.Second)
	if _, err := client.Send(t.Context(), request); err != nil {
		t.Fatalf("refresh Send: %v", err)
	}
	if tokenCalls != 2 {
		t.Fatalf("token calls = %d, want refresh before expiry", tokenCalls)
	}
}

func TestServiceTokenSingleFlightIsPerOrganization(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 3*time.Second)
	defer cancel()
	reached := make(chan string, 3)
	release := make(chan struct{})
	var releaseOnce sync.Once
	releaseAll := func() { releaseOnce.Do(func() { close(release) }) }
	defer releaseAll()
	var mu sync.Mutex
	calls := map[string]int{}
	authServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var request serviceTokenRequest
		if err := json.NewDecoder(r.Body).Decode(&request); err != nil {
			t.Error(err)
			return
		}
		mu.Lock()
		calls[request.OrgID]++
		mu.Unlock()
		reached <- request.OrgID
		select {
		case <-release:
		case <-r.Context().Done():
			return
		}
		_ = json.NewEncoder(w).Encode(serviceTokenResponse{
			Token: "token-" + request.OrgID, ExpiresAt: time.Now().UTC().Add(5 * time.Minute).Format(time.RFC3339),
		})
	}))
	defer authServer.Close()
	client := NewClient("http://unused", "internal", WithServicePrincipal(authServer.URL, "conversation-core", "credential"))

	results := make(chan *SendError, 3)
	for _, orgID := range []string{"org-1", "org-1", "org-2"} {
		go func(orgID string) {
			_, err := client.serviceToken(ctx, orgID)
			results <- err
		}(orgID)
	}
	seen := map[string]bool{}
	for range 2 {
		select {
		case orgID := <-reached:
			seen[orgID] = true
		case <-ctx.Done():
			releaseAll()
			t.Fatal("different organization token misses were serialized")
		}
	}
	if !seen["org-1"] || !seen["org-2"] {
		t.Fatalf("concurrent authorities reached = %#v", seen)
	}
	releaseAll()
	for range 3 {
		select {
		case err := <-results:
			if err != nil {
				t.Fatal(err)
			}
		case <-ctx.Done():
			t.Fatal("service-token single-flight did not complete within bound")
		}
	}
	mu.Lock()
	defer mu.Unlock()
	if calls["org-1"] != 1 || calls["org-2"] != 1 {
		t.Fatalf("authority calls = %#v, want one per organization", calls)
	}
}

func TestSendFailsBeforeProviderWhenServiceTokenCannotBeAcquired(t *testing.T) {
	tests := []struct {
		name          string
		status        int
		body          string
		wantCode      string
		wantSafeRetry bool
		redirect      bool
		wantTarget    bool
	}{
		{name: "credential rejected", status: http.StatusForbidden, body: `{"error":"forbidden"}`, wantCode: "auth_token_rejected"},
		{name: "authority unavailable", status: http.StatusServiceUnavailable, body: `{"error":"down"}`, wantCode: "auth_token_unavailable", wantSafeRetry: true},
		{name: "malformed success", status: http.StatusOK, body: `{"token":`, wantCode: "auth_token_invalid", wantSafeRetry: true},
		{name: "oversized success", status: http.StatusOK, body: `{"token":"` + strings.Repeat("x", 70<<10) + `"}`, wantCode: "auth_token_invalid", wantSafeRetry: true},
		{name: "redirect", status: http.StatusTemporaryRedirect, wantCode: "auth_token_unavailable", redirect: true, wantSafeRetry: true},
	}
	for _, testCase := range tests {
		t.Run(testCase.name, func(t *testing.T) {
			redirectTargetCalled := false
			redirectTarget := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
				redirectTargetCalled = true
				w.WriteHeader(http.StatusOK)
			}))
			defer redirectTarget.Close()
			authServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
				if testCase.redirect {
					w.Header().Set("Location", redirectTarget.URL)
				}
				w.WriteHeader(testCase.status)
				_, _ = w.Write([]byte(testCase.body))
			}))
			defer authServer.Close()
			providerCalls := 0
			integrationServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
				providerCalls++
				w.WriteHeader(http.StatusOK)
			}))
			defer integrationServer.Close()

			client := NewClient(integrationServer.URL, "internal-only-key",
				WithServicePrincipal(authServer.URL, "conversation-core", "service-credential-at-least-32-bytes"),
				WithWriteAttestor(&recordingAttestor{compact: "test-proof"}))
			_, err := client.Send(t.Context(), authorizedTestRequest(t, SendRequest{OrgID: "org-1", Provider: "slack", ConnectionID: "conn-1", ProviderThreadID: "C123", BodyText: "hello"}))
			if err == nil || IsTerminal(err) == testCase.wantSafeRetry || IsSafeToRetry(err) != testCase.wantSafeRetry || ErrorCode(err) != testCase.wantCode {
				t.Fatalf("Send error = %v, terminal=%v safeRetry=%v code=%q", err, IsTerminal(err), IsSafeToRetry(err), ErrorCode(err))
			}
			if providerCalls != 0 || redirectTargetCalled != testCase.wantTarget {
				t.Fatalf("provider calls=%d redirect target=%v, want no authority forwarding", providerCalls, redirectTargetCalled)
			}
		})
	}
}

// newTestClient points a Client at an httptest server with a fast timeout so
// the timeout test does not stall the suite.
func newTestClient(t *testing.T, baseURL string) *Client {
	t.Helper()
	authServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var request serviceTokenRequest
		if err := json.NewDecoder(r.Body).Decode(&request); err != nil || strings.TrimSpace(request.OrgID) == "" {
			w.WriteHeader(http.StatusBadRequest)
			return
		}
		_ = json.NewEncoder(w).Encode(serviceTokenResponse{
			Token:     "test-service-token-for-" + request.OrgID,
			ExpiresAt: time.Now().UTC().Add(5 * time.Minute).Format(time.RFC3339),
		})
	}))
	t.Cleanup(authServer.Close)
	return NewClient(baseURL, "test-key",
		WithServicePrincipal(authServer.URL, "conversation-core", "test-service-principal-credential-32-bytes"),
		WithWriteAttestor(&recordingAttestor{compact: "test-write-attestation"}),
		WithHTTPClient(&http.Client{Timeout: 200 * time.Millisecond}),
	)
}

func authorizedTestRequest(t *testing.T, request SendRequest) SendRequest {
	t.Helper()
	if request.ActorUserID == "" {
		request.ActorUserID = "test-user"
	}
	if request.AuthorizationKind == "" {
		request.AuthorizationKind = "human_intent"
	}
	if request.AuthorizationID == "" {
		request.AuthorizationID = "test-outintent"
	}
	if request.ActionID == "" {
		request.ActionID = request.AuthorizationID
	}
	if request.IdempotencyKey == "" {
		request.IdempotencyKey = "conversation:test-intent"
	}
	prepared, err := PrepareSend(request)
	if err == nil {
		request.PayloadSHA256 = prepared.PayloadSHA256
	}
	return request
}

func TestSend_Success_ReturnsProviderMessageID(t *testing.T) {
	var gotPath, gotAuthorization, gotKey, gotOrg, gotUser, gotOperation string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPath = r.URL.Path
		gotAuthorization = r.Header.Get("Authorization")
		gotKey = r.Header.Get("x-internal-api-key")
		gotOrg = r.Header.Get("x-org-id")
		gotUser = r.Header.Get("x-user-id")
		body, _ := io.ReadAll(r.Body)
		var parsed actionRequestBody
		_ = json.Unmarshal(body, &parsed)
		gotOperation = parsed.Operation
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"providerKey":"slack","operation":"message.send","result":{"ts":"1700000000.000100"}}}}`))
	}))
	defer srv.Close()

	cl := newTestClient(t, srv.URL)
	res, err := cl.Send(context.Background(), authorizedTestRequest(t, SendRequest{
		OrgID:            "org-1",
		ActorUserID:      "user-7",
		Provider:         "slack",
		ConnectionID:     "conn-9",
		ProviderThreadID: "C123",
		BodyText:         "hello",
	}))
	if err != nil {
		t.Fatalf("Send err = %v, want nil", err)
	}
	if res.ProviderMessageID != "1700000000.000100" {
		t.Errorf("provider_message_id = %q, want the slack ts", res.ProviderMessageID)
	}
	if gotPath != "/api/v1/connections/conn-9/actions" {
		t.Errorf("path = %q, want connections actions path", gotPath)
	}
	if gotAuthorization != "Bearer test-service-token-for-org-1" {
		t.Errorf("Authorization = %q, want tenant-bound service bearer", gotAuthorization)
	}
	if gotKey != "" || gotOrg != "" || gotUser != "" {
		t.Errorf("legacy key/org/user headers leaked = %q/%q/%q", gotKey, gotOrg, gotUser)
	}
	if gotOperation != "message.send" {
		t.Errorf("operation = %q, want message.send for slack", gotOperation)
	}
}

func TestSendForwardsAttestationAndIdempotencyContract(t *testing.T) {
	var got actionRequestBody
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if err := json.NewDecoder(r.Body).Decode(&got); err != nil {
			t.Fatalf("Decode request: %v", err)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"providerKey":"slack","operation":"message.send","result":{"ts":"1700000000.000100"}}}}`))
	}))
	defer srv.Close()

	_, err := newTestClient(t, srv.URL).Send(t.Context(), authorizedTestRequest(t, SendRequest{
		OrgID:            "org-1",
		Provider:         "slack",
		ConnectionID:     "conn-1",
		ProviderThreadID: "C123",
		BodyText:         "hello",
		AuthorizationID:  "human-reply-1",
		ActionID:         "human-reply-1",
		IdempotencyKey:   "conversation:org-1:human-reply-1",
	}))
	if err != nil {
		t.Fatalf("Send() error = %v", err)
	}
	if got.WriteAttestation != "test-write-attestation" || got.IdempotencyKey != "conversation:org-1:human-reply-1" {
		t.Fatalf("attestation/idempotency = %q/%q, want durable provider-write contract", got.WriteAttestation, got.IdempotencyKey)
	}
}

func TestSend_NestedSlackMessageTS(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"result":{"message":{"ts":"42.99"}}}}}`))
	}))
	defer srv.Close()
	cl := newTestClient(t, srv.URL)
	res, err := cl.Send(context.Background(), authorizedTestRequest(t, SendRequest{OrgID: "org-test", Provider: "slack", ConnectionID: "c1", ProviderThreadID: "C123", BodyText: "x"}))
	if err != nil {
		t.Fatalf("Send err = %v", err)
	}
	if res.ProviderMessageID != "42.99" {
		t.Errorf("nested message.ts = %q, want 42.99", res.ProviderMessageID)
	}
}

func TestSend_Success_EmptyBodyIsStillSuccess(t *testing.T) {
	// Graph sendMail returns 202 with no body — must still be a success, just
	// without a provider message id.
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusAccepted)
	}))
	defer srv.Close()
	cl := newTestClient(t, srv.URL)
	res, err := cl.Send(context.Background(), authorizedTestRequest(t, SendRequest{OrgID: "org-test", Provider: "microsoft", ConnectionID: "c1", BodyText: "x", To: []string{"a@b.no"}}))
	if err != nil {
		t.Fatalf("Send err = %v, want nil for 202", err)
	}
	if res.ProviderMessageID != "" {
		t.Errorf("provider_message_id = %q, want empty for no-body 202", res.ProviderMessageID)
	}
	if res.Operation != "mail.send" {
		t.Errorf("operation = %q, want mail.send for microsoft", res.Operation)
	}
}

func TestSend_RejectsMalformedOrFalseSuccess2xx(t *testing.T) {
	for _, testCase := range []struct {
		name string
		body string
	}{
		{name: "malformed", body: `{"success":`},
		{name: "explicit failure", body: `{"success":false,"error":{"code":"provider_rejected","message":"sensitive provider detail"}}`},
		{name: "empty", body: ``},
	} {
		t.Run(testCase.name, func(t *testing.T) {
			srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
				w.WriteHeader(http.StatusOK)
				_, _ = w.Write([]byte(testCase.body))
			}))
			defer srv.Close()

			_, err := newTestClient(t, srv.URL).Send(context.Background(), authorizedTestRequest(t, SendRequest{
				OrgID:            "org-test",
				Provider:         "slack",
				ConnectionID:     "c1",
				ProviderThreadID: "C123",
				BodyText:         "x",
			}))
			if err == nil {
				t.Fatal("Send() error = nil, want invalid-response failure")
			}
			var sendErr *SendError
			if !errors.As(err, &sendErr) || sendErr.Code != "invalid_response" {
				t.Fatalf("Send() error = %v, want SendError invalid_response", err)
			}
			if IsTerminal(err) {
				t.Fatalf("malformed post-provider 2xx classified terminal: %v", err)
			}
			if strings.Contains(err.Error(), "sensitive provider detail") {
				t.Fatalf("Send() error leaked provider detail: %v", err)
			}
		})
	}
}

func TestSend_OversizedPostProvider2xxIsAmbiguous(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte(strings.Repeat("x", actionResponseMaxBytes+1)))
	}))
	defer srv.Close()

	_, err := newTestClient(t, srv.URL).Send(t.Context(), authorizedTestRequest(t, SendRequest{
		OrgID: "org-test", Provider: "slack", ConnectionID: "c1", ProviderThreadID: "C123", BodyText: "x",
	}))
	if err == nil || IsTerminal(err) || ErrorCode(err) != "invalid_response" {
		t.Fatalf("Send error = %v, want ambiguous bounded-response failure", err)
	}
}

func TestSend_DoesNotForwardAuthorityAcrossRedirect(t *testing.T) {
	targetCalled := false
	target := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		targetCalled = true
		if r.Header.Get("x-internal-api-key") != "" || r.Header.Get("x-org-id") != "" {
			t.Errorf("delegated authority reached redirect target")
		}
		w.WriteHeader(http.StatusOK)
	}))
	defer target.Close()

	redirect := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Location", target.URL)
		w.WriteHeader(http.StatusTemporaryRedirect)
	}))
	defer redirect.Close()

	_, err := newTestClient(t, redirect.URL).Send(context.Background(), SendRequest{
		OrgID:            "org-1",
		Provider:         "slack",
		ConnectionID:     "c1",
		ProviderThreadID: "C123",
		BodyText:         "x",
	})
	if err == nil {
		t.Fatal("Send() error = nil, want redirect rejection")
	}
	if targetCalled {
		t.Fatal("redirect target was called")
	}
}

func TestSend_4xx_IsTerminal(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusBadRequest)
		_, _ = w.Write([]byte(`{"success":false,"error":{"code":"invalid_body","message":"bad"}}`))
	}))
	defer srv.Close()
	cl := newTestClient(t, srv.URL)
	_, err := cl.Send(context.Background(), authorizedTestRequest(t, SendRequest{OrgID: "org-test", Provider: "slack", ConnectionID: "c1", ProviderThreadID: "C123", BodyText: "x"}))
	if err == nil {
		t.Fatal("Send err = nil, want terminal error")
	}
	if !IsTerminal(err) {
		t.Errorf("4xx not classified terminal: %v", err)
	}
	if strings.Contains(err.Error(), "bad") {
		t.Fatalf("4xx error leaked upstream response text: %v", err)
	}
}

func TestSend_5xx_IsTransient(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusBadGateway)
		_, _ = w.Write([]byte(`{"success":false,"error":{"code":"upstream","message":"down"}}`))
	}))
	defer srv.Close()
	cl := newTestClient(t, srv.URL)
	_, err := cl.Send(context.Background(), authorizedTestRequest(t, SendRequest{OrgID: "org-test", Provider: "slack", ConnectionID: "c1", ProviderThreadID: "C123", BodyText: "x"}))
	if err == nil {
		t.Fatal("Send err = nil, want transient error")
	}
	if IsTerminal(err) {
		t.Errorf("5xx classified terminal, want transient: %v", err)
	}
	if IsSafeToRetry(err) {
		t.Errorf("generic 5xx was incorrectly marked pre-provider safe: %v", err)
	}
}

func TestSend_DefinitePreProvider503IsSafeToRetry(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusServiceUnavailable)
		_, _ = w.Write([]byte(`{"error":{"code":"action_pre_provider_retryable","message":"receipt was not reached"}}`))
	}))
	defer srv.Close()
	_, err := newTestClient(t, srv.URL).Send(t.Context(), authorizedTestRequest(t, SendRequest{
		OrgID: "org-test", Provider: "slack", ConnectionID: "c1", ProviderThreadID: "C123", BodyText: "x",
	}))
	if err == nil || IsTerminal(err) || !IsSafeToRetry(err) || ErrorCode(err) != "action_pre_provider_retryable" {
		t.Fatalf("Send error = %v terminal=%v safe=%v code=%q", err, IsTerminal(err), IsSafeToRetry(err), ErrorCode(err))
	}
}

func TestSend_DurableReceiptUnknown409IsAmbiguousNotTerminal(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusConflict)
		_, _ = w.Write([]byte(`{"success":false,"error":{"code":"action_outcome_unknown","message":"reconciliation required"}}`))
	}))
	defer srv.Close()

	_, err := newTestClient(t, srv.URL).Send(t.Context(), authorizedTestRequest(t, SendRequest{
		OrgID: "org-test", Provider: "slack", ConnectionID: "c1", ProviderThreadID: "C123", BodyText: "x",
	}))
	if err == nil {
		t.Fatal("Send err = nil, want reconciliation-required error")
	}
	if IsTerminal(err) {
		t.Fatalf("action_outcome_unknown classified terminal: %v", err)
	}
	if ErrorCode(err) != "action_outcome_unknown" {
		t.Fatalf("error code = %q", ErrorCode(err))
	}
}

func TestSend_Timeout_IsTransient(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		time.Sleep(500 * time.Millisecond) // exceeds the 200ms client timeout
		w.WriteHeader(http.StatusOK)
	}))
	defer srv.Close()
	cl := newTestClient(t, srv.URL)
	_, err := cl.Send(context.Background(), authorizedTestRequest(t, SendRequest{OrgID: "org-test", Provider: "slack", ConnectionID: "c1", ProviderThreadID: "C123", BodyText: "x"}))
	if err == nil {
		t.Fatal("Send err = nil, want transient timeout error")
	}
	if IsTerminal(err) {
		t.Errorf("timeout classified terminal, want transient: %v", err)
	}
}

func TestSend_UnsupportedProvider_IsTerminal(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		t.Error("server must not be hit for an unsupported provider")
		w.WriteHeader(http.StatusOK)
	}))
	defer srv.Close()
	cl := newTestClient(t, srv.URL)
	_, err := cl.Send(context.Background(), SendRequest{OrgID: "org-test", Provider: "fax", ConnectionID: "c1", BodyText: "x"})
	if err == nil || !IsTerminal(err) {
		t.Fatalf("unsupported provider err = %v, want terminal", err)
	}
}

func TestSend_NotConfigured_IsTerminal(t *testing.T) {
	cl := NewClient("", "")
	_, err := cl.Send(context.Background(), SendRequest{Provider: "slack", ConnectionID: "c1", ProviderThreadID: "C123", BodyText: "x"})
	if err == nil || !IsTerminal(err) {
		t.Fatalf("unconfigured client err = %v, want terminal", err)
	}
}

func TestSend_MissingOrganizationFailsBeforeAuthOrProvider(t *testing.T) {
	client := NewClient("https://integration.invalid", "internal-webhook-key",
		WithServicePrincipal("https://auth.invalid", "conversation-core", "service-credential-at-least-32-bytes"),
	)
	_, err := client.Send(t.Context(), SendRequest{Provider: "slack", ConnectionID: "c1", ProviderThreadID: "C123", BodyText: "x"})
	if err == nil || ErrorCode(err) != "missing_organization" || !IsTerminal(err) {
		t.Fatalf("Send error = %v, want terminal missing_organization", err)
	}
}

func TestSend_MissingConnection_IsTerminal(t *testing.T) {
	cl := NewClient("https://integration.local", "k")
	_, err := cl.Send(context.Background(), SendRequest{Provider: "slack", BodyText: "x"})
	if err == nil || !IsTerminal(err) {
		t.Fatalf("missing connection err = %v, want terminal", err)
	}
}

func TestSend_WhatsApp_UsesCloudAPIShape(t *testing.T) {
	var gotOp string
	var gotParams, gotBody map[string]any
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		raw, _ := io.ReadAll(r.Body)
		var parsed actionRequestBody
		_ = json.Unmarshal(raw, &parsed)
		gotOp, gotParams, gotBody = parsed.Operation, parsed.Params, parsed.Body
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"result":{"messages":[{"id":"wamid.HBgL123"}]}}}}`))
	}))
	defer srv.Close()

	res, err := newTestClient(t, srv.URL).Send(context.Background(), authorizedTestRequest(t, SendRequest{
		OrgID:            "org-test",
		Provider:         "whatsapp",
		ConnectionID:     "c1",
		ProviderThreadID: "1067xxxxphone:4790012345",
		BodyText:         "Hei!",
	}))
	if err != nil {
		t.Fatalf("whatsapp send err = %v", err)
	}
	if gotOp != "whatsapp.messages.send" {
		t.Errorf("operation = %q, want whatsapp.messages.send", gotOp)
	}
	if gotParams["phoneNumberId"] != "1067xxxxphone" {
		t.Errorf("phoneNumberId = %v, want the business id from the composite thread id", gotParams["phoneNumberId"])
	}
	if gotBody["to"] != "4790012345" || gotBody["type"] != "text" {
		t.Errorf("body = %#v, want to=recipient type=text", gotBody)
	}
	if txt, _ := gotBody["text"].(map[string]any); txt["body"] != "Hei!" {
		t.Errorf("text.body = %#v, want the message text", gotBody["text"])
	}
	if res.ProviderMessageID != "wamid.HBgL123" {
		t.Errorf("provider_message_id = %q, want the wamid from messages[0].id", res.ProviderMessageID)
	}
}

func TestSend_Messenger_UsesSendAPIShape(t *testing.T) {
	var gotOp string
	var gotParams, gotBody map[string]any
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		raw, _ := io.ReadAll(r.Body)
		var parsed actionRequestBody
		_ = json.Unmarshal(raw, &parsed)
		gotOp, gotParams, gotBody = parsed.Operation, parsed.Params, parsed.Body
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"result":{"message_id":"m_AG5Hc2"}}}}`))
	}))
	defer srv.Close()

	res, err := newTestClient(t, srv.URL).Send(context.Background(), authorizedTestRequest(t, SendRequest{
		OrgID:            "org-test",
		Provider:         "messenger",
		ConnectionID:     "c1",
		ProviderThreadID: "1094page:73940psid",
		BodyText:         "Takk for meldingen",
	}))
	if err != nil {
		t.Fatalf("messenger send err = %v", err)
	}
	if gotOp != "messenger.messages.send" {
		t.Errorf("operation = %q, want messenger.messages.send", gotOp)
	}
	if gotParams["pageId"] != "1094page" {
		t.Errorf("pageId = %v, want the page id from the composite thread id", gotParams["pageId"])
	}
	recip, _ := gotBody["recipient"].(map[string]any)
	if recip["id"] != "73940psid" {
		t.Errorf("recipient.id = %#v, want the PSID", gotBody["recipient"])
	}
	if res.ProviderMessageID != "m_AG5Hc2" {
		t.Errorf("provider_message_id = %q, want the messenger message_id", res.ProviderMessageID)
	}
}

func TestSend_WhatsApp_MissingBusinessID_IsTerminal(t *testing.T) {
	// No ":" in the thread id → no business phone-number id → cannot route.
	_, err := newTestClient(t, "http://unused").Send(context.Background(), SendRequest{
		OrgID:            "org-test",
		Provider:         "whatsapp",
		ConnectionID:     "c1",
		ProviderThreadID: "4790012345",
		BodyText:         "hi",
	})
	if err == nil || !IsTerminal(err) {
		t.Fatalf("err = %v, want terminal for missing business id", err)
	}
}

func TestSend_Discord_IsHonestlyUnsupported(t *testing.T) {
	_, err := newTestClient(t, "http://unused").Send(context.Background(), SendRequest{
		OrgID: "org-test", Provider: "discord", ConnectionID: "c1", BodyText: "hi",
	})
	if err == nil || !IsTerminal(err) {
		t.Fatalf("err = %v, want terminal for discord", err)
	}
}

func TestSend_PerProviderOperationMapping(t *testing.T) {
	cases := map[string]string{
		"microsoft": "mail.send",
		"slack":     "message.send",
		"google":    "gmail.send",
	}
	for provider, wantOp := range cases {
		var gotOp string
		srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			body, _ := io.ReadAll(r.Body)
			var parsed actionRequestBody
			_ = json.Unmarshal(body, &parsed)
			gotOp = parsed.Operation
			_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"result":{"id":"m1"}}}}`))
		}))
		_, err := newTestClient(t, srv.URL).Send(context.Background(), authorizedTestRequest(t, SendRequest{
			OrgID: "org-test", Provider: provider, ConnectionID: "c1", ProviderThreadID: "C123", BodyText: "hi", To: []string{"x@y.no"},
		}))
		srv.Close()
		if err != nil {
			t.Fatalf("provider %s send err = %v", provider, err)
		}
		if !strings.EqualFold(gotOp, wantOp) {
			t.Errorf("provider %s operation = %q, want %q", provider, gotOp, wantOp)
		}
	}
}

func TestSupportsSend(t *testing.T) {
	// Every provider buildSendOperation maps must be reported as sendable —
	// including whatsapp and messenger, the channels the human-reply fix targets.
	// Case/whitespace are normalized like buildSendOperation does.
	for _, p := range []string{"whatsapp", "messenger", "instagram", "microsoft", "slack", "google", "WhatsApp", " Messenger "} {
		if !SupportsSend(p) {
			t.Errorf("SupportsSend(%q) = false, want true", p)
		}
	}
	// discord is honestly unsupported; a plain email inbox / unknown provider has
	// no send op and must stay store-only rather than erroring.
	for _, p := range []string{"discord", "email", "", "fax", "unknown"} {
		if SupportsSend(p) {
			t.Errorf("SupportsSend(%q) = true, want false", p)
		}
	}
}

func TestSend_Instagram_ResolvesLinkedPageServerSide(t *testing.T) {
	var gotOp string
	var gotParams, gotBody map[string]any
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		raw, _ := io.ReadAll(r.Body)
		var parsed actionRequestBody
		_ = json.Unmarshal(raw, &parsed)
		gotOp, gotParams, gotBody = parsed.Operation, parsed.Params, parsed.Body
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"result":{"message_id":"ig_m_1"}}}}`))
	}))
	defer srv.Close()

	res, err := newTestClient(t, srv.URL).Send(context.Background(), authorizedTestRequest(t, SendRequest{
		OrgID:            "org-test",
		Provider:         "instagram",
		ConnectionID:     "c1",
		ProviderThreadID: "17841400000000000:893000000000001",
		BodyText:         "Ja, det er på lager!",
	}))
	if err != nil {
		t.Fatalf("instagram send err = %v", err)
	}
	if gotOp != "instagram.messages.send" {
		t.Errorf("operation = %q, want instagram.messages.send", gotOp)
	}
	if gotParams["igAccountId"] != "17841400000000000" {
		t.Errorf("igAccountId = %v, want the IG business-account id from the composite thread id", gotParams["igAccountId"])
	}
	recip, _ := gotBody["recipient"].(map[string]any)
	if recip["id"] != "893000000000001" {
		t.Errorf("recipient.id = %#v, want the IGSID", gotBody["recipient"])
	}
	if res.ProviderMessageID != "ig_m_1" {
		t.Errorf("ProviderMessageID = %q", res.ProviderMessageID)
	}
}

func TestBuildSendOperation_SlackThreadRefSplit(t *testing.T) {
	// Threaded reply: composite channel:thread_ts must split — the previous
	// behavior sent the whole composite as thread_ts (invalid ts).
	op, params, body, err := buildSendOperation(SendRequest{
		Provider:         "slack",
		ProviderThreadID: "C0GENERAL:1751968800.000100",
		BodyText:         "svar",
	})
	if err != nil {
		t.Fatalf("threaded: %v", err)
	}
	if op != "message.send" {
		t.Errorf("operation = %q", op)
	}
	if params["channel"] != "C0GENERAL" || body["channel"] != "C0GENERAL" {
		t.Errorf("channel = %v", body["channel"])
	}
	if body["thread_ts"] != "1751968800.000100" {
		t.Errorf("thread_ts = %v, want the parent ts only", body["thread_ts"])
	}

	// Top-level reply: bare channel ref, no thread_ts at all.
	_, params, body, err = buildSendOperation(SendRequest{
		Provider:         "slack",
		ProviderThreadID: "C0GENERAL",
		BodyText:         "svar",
	})
	if err != nil {
		t.Fatalf("top-level: %v", err)
	}
	if params["channel"] != "C0GENERAL" {
		t.Errorf("channel = %v", params["channel"])
	}
	if _, present := body["thread_ts"]; present {
		t.Error("thread_ts must be absent for top-level channel replies")
	}
}

func TestBuildSendOperation_SlackWithoutChannelErrors(t *testing.T) {
	_, _, _, err := buildSendOperation(SendRequest{Provider: "slack", BodyText: "x"})
	if err == nil {
		t.Fatal("expected an addressing error without a channel ref")
	}
	// The addressing error must NOT mark slack unsupported — SupportsSend
	// derives from ErrUnsupportedProvider specifically.
	if !SupportsSend("slack") {
		t.Fatal("SupportsSend(slack) must remain true")
	}
}
