package notifyclient

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

// TestCanonicalStringAndSign_GoldenVector pins canonicalString and sign
// against an HMAC-SHA256 value computed INDEPENDENTLY of this package: a
// standalone throwaway Go program (crypto/sha256 + crypto/hmac + a hand-typed
// canonical join, not a call into this package) produced the two "want"
// constants below for this exact set of inputs. That is the only trustworthy
// way to test a signer — asserting sign(...) == sign(...) would just prove
// the function is deterministic, not that it agrees with notification-core's
// verifier.
//
// If canonicalString's field order, the "v2" prefix, or the join separator
// ever changes, this test must fail; do not "fix" it by pasting in whatever
// the new implementation happens to produce — rederive the vector
// independently first.
func TestCanonicalStringAndSign_GoldenVector(t *testing.T) {
	body := []byte(`{"organization_id":"org_test123","idempotency_key":"run-watch:org_test123:run_1:user_1:RUN_COMPLETED","recipient":{"kind":"user","id":"user_1"},"type":"modelplane.run_completed","payload":{"run_id":"run_1","title":"Run completed","body":"Your run finished."}}`)

	const wantDigest = "vEVsA974n5rbwH11jLrCHHrCbximmk3eWiTKvdkDs7k"
	if digest := bodyDigest(body); digest != wantDigest {
		t.Fatalf("bodyDigest = %q, want %q", digest, wantDigest)
	}

	canonical := canonicalString(canonicalFields{
		ServiceID:      "capability-core",
		Audience:       "notification-core",
		Timestamp:      "2026-08-18T12:34:56Z",
		Nonce:          "0123456789abcdefghij_-AB",
		Method:         "POST",
		URI:            "/notification-requests",
		UserID:         "",
		OrganizationID: "org_test123",
		Role:           "",
		BodySHA256:     wantDigest,
	})
	const wantCanonical = "v2\ncapability-core\nnotification-core\n2026-08-18T12:34:56Z\n" +
		"0123456789abcdefghij_-AB\nPOST\n/notification-requests\n\norg_test123\n\n" +
		"vEVsA974n5rbwH11jLrCHHrCbximmk3eWiTKvdkDs7k"
	if canonical != wantCanonical {
		t.Fatalf("canonicalString =\n%q\nwant\n%q", canonical, wantCanonical)
	}

	secret := []byte("unit-test-shared-secret-value-32bytes-minimum")
	const wantSignature = "36xkt47zOyRw9icDYBnOUour4sXoDcTSWuhTjStYm20"
	if signature := sign(secret, canonical); signature != wantSignature {
		t.Fatalf("sign = %q, want %q (independently hand-derived HMAC-SHA256, base64.RawURLEncoding)", signature, wantSignature)
	}
}

func TestNew_DisabledWithoutBaseURLOrToken(t *testing.T) {
	for _, test := range []struct {
		name  string
		url   string
		token string
	}{
		{"empty url", "", "a-token-that-is-long-enough-to-be-real"},
		{"empty token", "http://notification-core:3140", ""},
		{"both blank", "  ", "  "},
	} {
		t.Run(test.name, func(t *testing.T) {
			client, enabled := New(test.url, test.token, nil)
			if enabled || client != nil {
				t.Fatalf("New(%q, %q) = (%v, %v), want (nil, false)", test.url, test.token, client, enabled)
			}
		})
	}
}

func TestNew_EnabledTrimsTrailingSlash(t *testing.T) {
	client, enabled := New("http://notification-core:3140/", "a-real-service-token-value", nil)
	if !enabled || client == nil {
		t.Fatalf("New(...) = (%v, %v), want an enabled client", client, enabled)
	}
	if client.BaseURL != "http://notification-core:3140" {
		t.Fatalf("BaseURL = %q, want trailing slash trimmed", client.BaseURL)
	}
	if client.HTTP == nil {
		t.Fatal("New must install a default HTTP client")
	}
}

// TestAccept_SignsAndSendsExpectedRequest exercises the whole Accept path
// against a real httptest.Server, capturing the wire request to check: (1)
// the transmitted body is exactly one marshal of the Request (round-trips to
// the same fields, no Source leak), and (2) the body-sha256/signature
// headers are internally consistent with the ACTUAL captured bytes — i.e.
// Accept reused the same marshaled bytes for both the digest and the body,
// rather than risking a second marshal that could reorder the Payload map.
func TestAccept_SignsAndSendsExpectedRequest(t *testing.T) {
	var (
		gotMethod  string
		gotPath    string
		gotHeaders http.Header
		gotBody    []byte
	)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotMethod = r.Method
		gotPath = r.URL.Path
		gotHeaders = r.Header.Clone()
		var err error
		gotBody, err = io.ReadAll(r.Body)
		if err != nil {
			t.Errorf("read body: %v", err)
		}
		w.WriteHeader(http.StatusCreated)
	}))
	defer server.Close()

	fixedTime := time.Date(2026, 8, 18, 12, 34, 56, 0, time.UTC)
	client := &Client{
		BaseURL:      server.URL,
		ServiceToken: "unit-test-shared-secret-value-32bytes-minimum",
		HTTP:         server.Client(),
		Now:          func() time.Time { return fixedTime },
		Nonce:        func() (string, error) { return "0123456789abcdefghij_-AB", nil },
	}

	req := Request{
		OrganizationID: "org_test123",
		IdempotencyKey: "run-watch:org_test123:run_1:user_1:RUN_COMPLETED",
		Recipient:      Recipient{Kind: RecipientKindUser, ID: "user_1"},
		Type:           "modelplane.run_completed",
		Payload: map[string]any{
			"run_id": "run_1",
			"title":  "Run completed",
			"body":   "Your run finished.",
		},
	}
	if err := client.Accept(context.Background(), req); err != nil {
		t.Fatalf("Accept: %v", err)
	}

	// Asserted against the literal path, not the notificationRequestsPath
	// constant: a constant compared to itself can never catch that constant
	// pointing at the wrong endpoint. notification-core mounts this route
	// inside its versioned group (server.go: router.Group("/api/v1", ...)),
	// not at the bare path.
	const wantPath = "/api/v1/notification-requests"
	if gotMethod != http.MethodPost || gotPath != wantPath {
		t.Fatalf("request line = %s %s, want POST %s", gotMethod, gotPath, wantPath)
	}
	if gotHeaders.Get(headerServiceID) != "capability-core" {
		t.Fatalf("%s = %q, want capability-core", headerServiceID, gotHeaders.Get(headerServiceID))
	}
	if gotHeaders.Get(headerOrganizationID) != "org_test123" {
		t.Fatalf("%s = %q, want org_test123", headerOrganizationID, gotHeaders.Get(headerOrganizationID))
	}
	if gotHeaders.Get(headerTimestamp) != "2026-08-18T12:34:56Z" {
		t.Fatalf("%s = %q, want RFC3339 UTC", headerTimestamp, gotHeaders.Get(headerTimestamp))
	}
	if gotHeaders.Get(headerNonce) != "0123456789abcdefghij_-AB" {
		t.Fatalf("%s = %q", headerNonce, gotHeaders.Get(headerNonce))
	}
	if gotHeaders.Get("x-user-id") != "" || gotHeaders.Get("x-user-role") != "" {
		t.Fatal("x-user-id/x-user-role must never be sent by this client")
	}

	// The digest header must bind the bytes ACTUALLY transmitted, not some
	// other marshal of an equal-looking struct.
	if want := bodyDigest(gotBody); gotHeaders.Get(headerBodySHA256) != want {
		t.Fatalf("%s = %q, does not match sha256 of the transmitted body (%q)", headerBodySHA256, gotHeaders.Get(headerBodySHA256), want)
	}
	// The signature must verify against the canonical string built from the
	// ACTUAL header values Accept sent, using the same secret.
	wantCanonical := canonicalString(canonicalFields{
		ServiceID:      "capability-core",
		Audience:       "notification-core",
		Timestamp:      gotHeaders.Get(headerTimestamp),
		Nonce:          gotHeaders.Get(headerNonce),
		Method:         http.MethodPost,
		URI:            notificationRequestsPath,
		UserID:         "",
		OrganizationID: "org_test123",
		Role:           "",
		BodySHA256:     gotHeaders.Get(headerBodySHA256),
	})
	wantSignature := sign([]byte(client.ServiceToken), wantCanonical)
	if gotHeaders.Get(headerSignature) != wantSignature {
		t.Fatalf("%s = %q, want %q (derived from the captured wire values)", headerSignature, gotHeaders.Get(headerSignature), wantSignature)
	}

	var decoded map[string]any
	if err := json.Unmarshal(gotBody, &decoded); err != nil {
		t.Fatalf("decode transmitted body: %v", err)
	}
	if decoded["organization_id"] != "org_test123" || decoded["type"] != "modelplane.run_completed" {
		t.Fatalf("transmitted body = %v", decoded)
	}
	if _, hasSource := decoded["source"]; hasSource {
		t.Fatal("client must never send a source field; notification-core sets it server-side from the principal")
	}
}

func TestAccept_RequiresOrganizationID(t *testing.T) {
	client := &Client{BaseURL: "http://unused.invalid", ServiceToken: "a-real-service-token-value"}
	err := client.Accept(context.Background(), Request{Type: "modelplane.run_completed"})
	if err == nil || !strings.Contains(err.Error(), "organization_id") {
		t.Fatalf("Accept without organization_id = %v, want an organization_id error", err)
	}
}

func TestAccept_NilClientIsDisabled(t *testing.T) {
	var client *Client
	if err := client.Accept(context.Background(), Request{OrganizationID: "org-1"}); err == nil {
		t.Fatal("Accept on a nil (disabled) client must return an error, not panic or silently succeed")
	}
}

func TestAccept_NonSuccessStatusIsError(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		http.Error(w, `{"error":"delegated organization scope required"}`, http.StatusForbidden)
	}))
	defer server.Close()

	client := &Client{
		BaseURL:      server.URL,
		ServiceToken: "unit-test-shared-secret-value-32bytes-minimum",
		HTTP:         server.Client(),
	}
	err := client.Accept(context.Background(), Request{
		OrganizationID: "org-1",
		Recipient:      Recipient{Kind: RecipientKindUser, ID: "user-1"},
		Type:           "modelplane.run_completed",
	})
	if err == nil {
		t.Fatal("expected a non-2xx response to be an error")
	}
}
