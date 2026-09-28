package mailtask

import (
	"context"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestHTTPAliasResolverSendsTheConfiguredServiceCredential(t *testing.T) {
	var gotPath, gotServiceID, gotServiceToken string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPath = r.URL.Path + "?" + r.URL.RawQuery
		gotServiceID = r.Header.Get("X-Service-Id")
		gotServiceToken = r.Header.Get("X-Service-Token")
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"org_id":"org-1","user_id":"user-1","verified_email":"owner@example.com"}`))
	}))
	defer server.Close()

	resolver := &HTTPAliasResolver{BaseURL: server.URL, ServiceID: "integration-mail-task-worker", ServiceToken: "the-token"}
	resolved, ok, err := resolver.Resolve(context.Background(), "abc def", "provider-msg-1") // a space to prove query-escaping
	if err != nil {
		t.Fatalf("Resolve() error = %v", err)
	}
	if !ok {
		t.Fatal("Resolve() ok = false, want true")
	}
	if resolved.OrgID != "org-1" || resolved.UserID != "user-1" || resolved.VerifiedEmail != "owner@example.com" {
		t.Fatalf("resolved = %+v", resolved)
	}
	if gotPath != "/api/v1/internal/mail-task/resolve?alias=abc+def&provider_message_id=provider-msg-1" {
		t.Fatalf("path = %q", gotPath)
	}
	if gotServiceID != "integration-mail-task-worker" || gotServiceToken != "the-token" {
		t.Fatalf("service credential headers = %q/%q", gotServiceID, gotServiceToken)
	}
}

// TestHTTPAliasResolverPassesTheThreadDecisionThroughUnmodified is the
// delegated-identity design's own resolver-level exact test: `thread_decision`
// passes through `HTTPAliasResolver.Resolve` unmodified (Gap B §2.10).
func TestHTTPAliasResolverPassesTheThreadDecisionThroughUnmodified(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"org_id":"org-1","user_id":"user-1","verified_email":"owner@example.com","thread_decision":"mail-task-v1.k.p.s"}`))
	}))
	defer server.Close()

	resolver := &HTTPAliasResolver{BaseURL: server.URL, ServiceID: "svc", ServiceToken: "tok"}
	resolved, ok, err := resolver.Resolve(context.Background(), "abc123", "provider-msg-1")
	if err != nil || !ok {
		t.Fatalf("Resolve() = (%+v, %v, %v)", resolved, ok, err)
	}
	if resolved.ThreadDecision != "mail-task-v1.k.p.s" {
		t.Fatalf("ThreadDecision = %q, want it passed through verbatim", resolved.ThreadDecision)
	}
}

// TestHTTPAliasResolverOmitsTheProviderMessageIDQueryParamWhenAbsent pins
// backward compatibility: a caller with no providerMessageID (there is none
// today besides handler.go's own call) gets EXACTLY the original query
// string, never an empty trailing parameter.
func TestHTTPAliasResolverOmitsTheProviderMessageIDQueryParamWhenAbsent(t *testing.T) {
	var gotPath string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPath = r.URL.Path + "?" + r.URL.RawQuery
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"org_id":"org-1","user_id":"user-1","verified_email":"owner@example.com"}`))
	}))
	defer server.Close()

	resolver := &HTTPAliasResolver{BaseURL: server.URL, ServiceID: "svc", ServiceToken: "tok"}
	if _, _, err := resolver.Resolve(context.Background(), "abc123", ""); err != nil {
		t.Fatalf("Resolve() error = %v", err)
	}
	if gotPath != "/api/v1/internal/mail-task/resolve?alias=abc123" {
		t.Fatalf("path = %q, want no provider_message_id param", gotPath)
	}
}

func TestHTTPAliasResolverTreats404AsGenuinelyNotFoundWithoutAnError(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusNotFound)
	}))
	defer server.Close()

	resolver := &HTTPAliasResolver{BaseURL: server.URL, ServiceID: "svc", ServiceToken: "tok"}
	resolved, ok, err := resolver.Resolve(context.Background(), "does-not-exist", "provider-msg-1")
	if err != nil {
		t.Fatalf("Resolve() error = %v, want nil for a genuine 404", err)
	}
	if ok {
		t.Fatal("Resolve() ok = true, want false for a 404")
	}
	if resolved != (ResolvedAlias{}) {
		t.Fatalf("resolved = %+v, want zero value", resolved)
	}
}

// TestHTTPAliasResolverTreatsAnUnexpectedStatusAsAnErrorNotNotFound is the
// regression test for the bug this file's Resolve fix addresses: a
// user-core outage (500/503/anything but 200 or 404) used to collapse into
// the same (false, nil) as a genuine not-found, which made
// HandleInboundPostmark answer Postmark with a 200 and silently, permanently
// drop the email instead of letting Postmark's webhook retry recover once
// user-core is healthy again.
func TestHTTPAliasResolverTreatsAnUnexpectedStatusAsAnErrorNotNotFound(t *testing.T) {
	for _, status := range []int{http.StatusInternalServerError, http.StatusServiceUnavailable, http.StatusForbidden, http.StatusBadGateway} {
		t.Run(http.StatusText(status), func(t *testing.T) {
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				w.WriteHeader(status)
			}))
			defer server.Close()

			resolver := &HTTPAliasResolver{BaseURL: server.URL, ServiceID: "svc", ServiceToken: "tok"}
			resolved, ok, err := resolver.Resolve(context.Background(), "abc123", "provider-msg-1")
			if err == nil {
				t.Fatalf("Resolve() error = nil for HTTP %d, want a non-nil error so the caller retries instead of dropping the email", status)
			}
			if ok {
				t.Fatal("Resolve() ok = true, want false")
			}
			if resolved != (ResolvedAlias{}) {
				t.Fatalf("resolved = %+v, want zero value", resolved)
			}
		})
	}
}

func TestHTTPAliasResolverTreatsAMalformedSuccessBodyAsNotFound(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		// 200 but missing verified_email -- must not resolve as if it were a
		// complete, live alias.
		_, _ = w.Write([]byte(`{"org_id":"org-1","user_id":"user-1"}`))
	}))
	defer server.Close()

	resolver := &HTTPAliasResolver{BaseURL: server.URL, ServiceID: "svc", ServiceToken: "tok"}
	_, ok, err := resolver.Resolve(context.Background(), "abc123", "provider-msg-1")
	if err != nil {
		t.Fatalf("Resolve() error = %v, want nil", err)
	}
	if ok {
		t.Fatal("Resolve() ok = true, want false for an incomplete success body")
	}
}

func TestHTTPAliasResolverTransportFailureIsAnError(t *testing.T) {
	// Nothing listening on this port -- the request fails at the transport
	// level (connection refused), which must reach the caller as an error
	// exactly like an unexpected HTTP status does, never silently as "not
	// found".
	resolver := &HTTPAliasResolver{BaseURL: "http://127.0.0.1:1", ServiceID: "svc", ServiceToken: "tok"}
	_, ok, err := resolver.Resolve(context.Background(), "abc123", "provider-msg-1")
	if err == nil {
		t.Fatal("Resolve() error = nil, want a transport error")
	}
	if ok {
		t.Fatal("Resolve() ok = true, want false")
	}
}

func TestHTTPAliasResolverIsDisabledWithoutABaseURL(t *testing.T) {
	resolver := &HTTPAliasResolver{}
	resolved, ok, err := resolver.Resolve(context.Background(), "abc123", "provider-msg-1")
	if err != nil || ok || resolved != (ResolvedAlias{}) {
		t.Fatalf("Resolve() = (%+v, %v, %v), want the zero value with no error", resolved, ok, err)
	}
}
