package http

import (
	"context"
	"errors"
	"io"
	stdhttp "net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

const testMembershipAuthorityToken = "0123456789abcdef0123456789abcdef"

func TestNewServerConfiguresCanonicalMembershipAuthorityAndRejectsRedirects(t *testing.T) {
	t.Setenv("AUTH_SERVICE_URL", " http://auth-core.example.test/ ")
	t.Setenv("USER_CORE_MEMBERSHIP_SERVICE_TOKEN", testMembershipAuthorityToken)
	t.Setenv("ORG_SERVICE_URL", "http://org-core.example.test/")
	t.Setenv("INTERNAL_API_KEY", "0123456789abcdef0123456789abcdef")

	server := NewServer(nil, nil, nil, nil, "3012")
	if server.authMembershipService != "http://auth-core.example.test" || server.authMembershipToken != testMembershipAuthorityToken {
		t.Fatalf("membership config = %q/token-present:%v", server.authMembershipService, server.authMembershipToken != "")
	}
	redirectErr := server.httpClient.CheckRedirect(&stdhttp.Request{}, []*stdhttp.Request{{}})
	if !errors.Is(redirectErr, stdhttp.ErrUseLastResponse) {
		t.Fatalf("redirect error = %v, want ErrUseLastResponse", redirectErr)
	}
}

func TestNewServerUsesFailClosedInternalDefaults(t *testing.T) {
	t.Setenv("AUTH_SERVICE_URL", "")
	t.Setenv("USER_CORE_MEMBERSHIP_SERVICE_TOKEN", "")
	t.Setenv("ORG_SERVICE_URL", "")
	t.Setenv("INTERNAL_API_KEY", "")
	t.Setenv("INTERNAL_SERVICE_SECRET", testMembershipAuthorityToken)

	server := NewServer(nil, nil, nil, nil, "3012")
	if server.authMembershipService != "http://auth-core:3011" || server.orgService != "http://org-core:8080" {
		t.Fatalf("default services = %q/%q", server.authMembershipService, server.orgService)
	}
	if server.internalKey != testMembershipAuthorityToken || server.authMembershipToken != "" {
		t.Fatalf("default credential selection is incorrect")
	}
}

func membershipServer(t *testing.T, handler stdhttp.HandlerFunc) *Server {
	t.Helper()
	upstream := httptest.NewServer(handler)
	t.Cleanup(upstream.Close)
	return &Server{
		httpClient:            upstream.Client(),
		authMembershipService: upstream.URL,
		authMembershipToken:   testMembershipAuthorityToken,
	}
}

func TestResolveMembershipUsesExactCanonicalAuthRole(t *testing.T) {
	server := membershipServer(t, func(w stdhttp.ResponseWriter, r *stdhttp.Request) {
		if r.URL.Path != "/api/v1/internal/membership/decision" || r.Method != stdhttp.MethodPost {
			t.Fatalf("request = %s %s, want canonical membership decision endpoint", r.Method, r.URL.Path)
		}
		if got := r.Header.Get("X-User-Core-Membership-Token"); got != testMembershipAuthorityToken {
			t.Fatalf("membership token = %q, want configured dedicated credential", got)
		}
		body, err := io.ReadAll(r.Body)
		if err != nil {
			t.Fatal(err)
		}
		if got := strings.TrimSpace(string(body)); got != `{"orgId":"org-1","userId":"user-1"}` {
			t.Fatalf("body = %s", got)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"version":"v1","member":true,"role":"viewer"}`))
	})

	orgID, role, err := server.resolveMembershipFromAuthCore(context.Background(), "user-1", "org-1")
	if err != nil {
		t.Fatalf("resolveMembershipFromAuthCore() error = %v", err)
	}
	if orgID != "org-1" || role != "viewer" {
		t.Fatalf("membership = (%q, %q), want (org-1, viewer)", orgID, role)
	}
}

func TestResolveMembershipRetriesOneTransientAuthorityTransportFailure(t *testing.T) {
	attempts := 0
	server := membershipServer(t, func(w stdhttp.ResponseWriter, _ *stdhttp.Request) {
		attempts++
		if attempts == 1 {
			connection, _, err := w.(stdhttp.Hijacker).Hijack()
			if err != nil {
				t.Fatalf("hijack transient authority connection: %v", err)
			}
			_ = connection.Close()
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"version":"v1","member":true,"role":"member"}`))
	})

	orgID, role, err := server.resolveMembershipFromAuthCore(context.Background(), "user-1", "org-1")
	if err != nil {
		t.Fatalf("resolveMembershipFromAuthCore() error = %v", err)
	}
	if orgID != "org-1" || role != "member" {
		t.Fatalf("membership = (%q, %q), want (org-1, member)", orgID, role)
	}
	if attempts != 2 {
		t.Fatalf("authority attempts = %d, want exactly 2", attempts)
	}
}

func TestResolveMembershipRequiresExplicitOrganization(t *testing.T) {
	server := membershipServer(t, func(stdhttp.ResponseWriter, *stdhttp.Request) {
		t.Fatal("canonical authority must not be called without an explicit active organization")
	})

	_, _, err := server.resolveMembershipFromAuthCore(context.Background(), "user-1", "")
	if !errors.Is(err, ErrMembershipNotFound) {
		t.Fatalf("error = %v, want ErrMembershipNotFound", err)
	}
}

func TestResolveMembershipRejectsMissingExactMember(t *testing.T) {
	server := membershipServer(t, func(w stdhttp.ResponseWriter, _ *stdhttp.Request) {
		_, _ = w.Write([]byte(`{"version":"v1","member":false,"role":null}`))
	})

	_, role, err := server.resolveMembershipFromAuthCore(context.Background(), "user-1", "org-forged")
	if !errors.Is(err, ErrMembershipNotFound) || role != "" {
		t.Fatalf("role/error = %q/%v, want empty role and ErrMembershipNotFound", role, err)
	}
}

func TestResolveMembershipFailsClosedWhenCanonicalAuthorityDegrades(t *testing.T) {
	tests := []struct {
		name    string
		handler stdhttp.HandlerFunc
	}{
		{
			name: "non-200",
			handler: func(w stdhttp.ResponseWriter, _ *stdhttp.Request) {
				w.WriteHeader(stdhttp.StatusServiceUnavailable)
			},
		},
		{
			name: "malformed",
			handler: func(w stdhttp.ResponseWriter, _ *stdhttp.Request) {
				_, _ = w.Write([]byte(`{"member":`))
			},
		},
		{
			name: "unknown role",
			handler: func(w stdhttp.ResponseWriter, _ *stdhttp.Request) {
				_, _ = w.Write([]byte(`{"version":"v1","member":true,"role":"superuser"}`))
			},
		},
		{
			name: "missing version",
			handler: func(w stdhttp.ResponseWriter, _ *stdhttp.Request) {
				_, _ = w.Write([]byte(`{"member":true,"role":"member"}`))
			},
		},
		{
			name: "trailing JSON",
			handler: func(w stdhttp.ResponseWriter, _ *stdhttp.Request) {
				_, _ = w.Write([]byte(`{"version":"v1","member":true,"role":"member"}{"member":false}`))
			},
		},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			server := membershipServer(t, test.handler)
			if _, role, err := server.resolveMembershipFromAuthCore(context.Background(), "user-1", "org-1"); err == nil || role != "" {
				t.Fatalf("role/error = %q/%v, want empty role and authority error", role, err)
			}
		})
	}
}

func TestMembershipAuthorityRedirectIsNotFollowedAndCredentialIsNotLeaked(t *testing.T) {
	redirectTargetCalled := false
	redirectTarget := httptest.NewServer(stdhttp.HandlerFunc(func(_ stdhttp.ResponseWriter, r *stdhttp.Request) {
		redirectTargetCalled = true
		if r.Header.Get("X-User-Core-Membership-Token") != "" {
			t.Fatal("dedicated membership credential leaked across redirect")
		}
	}))
	defer redirectTarget.Close()

	server := membershipServer(t, func(w stdhttp.ResponseWriter, _ *stdhttp.Request) {
		stdhttp.Redirect(w, &stdhttp.Request{}, redirectTarget.URL, stdhttp.StatusTemporaryRedirect)
	})
	server.httpClient.CheckRedirect = func(*stdhttp.Request, []*stdhttp.Request) error {
		return stdhttp.ErrUseLastResponse
	}

	_, _, err := server.resolveMembershipFromAuthCore(context.Background(), "user-1", "org-1")
	if err == nil {
		t.Fatal("redirecting authority must fail closed")
	}
	if redirectTargetCalled {
		t.Fatal("redirect target must not be called")
	}
}
