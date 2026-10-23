package server

import (
	"bytes"
	"context"
	"log/slog"
	"strings"
	"testing"

	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
)

// captureLog swaps the default logger for a buffer for one test.
func captureLog(t *testing.T) *bytes.Buffer {
	t.Helper()
	var buf bytes.Buffer
	previous := slog.Default()
	slog.SetDefault(slog.New(slog.NewJSONHandler(&buf, nil)))
	t.Cleanup(func() { slog.SetDefault(previous) })
	return &buf
}

func TestAcquireGrant_LogsOneLineWithTheGrantIdAndNoDomain(t *testing.T) {
	buf := captureLog(t)
	srv := testServerFor(grant.NewMemoryRepository(), userPrincipal(false))
	resp := acquire(t, srv, &AcquireGrantRequest{AllowedDomains: []string{"secret-host.example.com"}})

	out := buf.String()
	if n := strings.Count(out, `"browser grant issued"`); n != 1 {
		t.Fatalf("want exactly one issue line, got %d: %s", n, out)
	}
	if !strings.Contains(out, `"method":"AcquireGrant"`) || !strings.Contains(out, resp.GetGrantId()) {
		t.Fatalf("the line must name the method and the grant id: %s", out)
	}
	for _, leak := range []string{"secret-host", "example.com", "org1", "user1"} {
		if strings.Contains(out, leak) {
			t.Fatalf("the log line carries %q: %s", leak, out)
		}
	}
}

func TestAcquireGrant_ZdrLogLineHasNeitherDomainNorGrantId(t *testing.T) {
	buf := captureLog(t)
	srv := testServerFor(grant.NewMemoryRepository(), userPrincipal(true))
	resp := acquire(t, srv, &AcquireGrantRequest{AllowedDomains: []string{"secret-host.example.com"}})
	if !strings.HasPrefix(resp.GetGrantId(), "z_") {
		t.Fatalf("expected a ZDR grant, got %q", resp.GetGrantId())
	}
	out := buf.String()
	if !strings.Contains(out, `"zdr":true`) {
		t.Fatalf("the line must say zdr=true: %s", out)
	}
	for _, leak := range []string{"secret-host", "example.com", resp.GetGrantId(), "org1", "user1"} {
		if strings.Contains(out, leak) {
			t.Fatalf("the ZDR log line carries %q: %s", leak, out)
		}
	}
}

func TestRevokeGrant_LogsOneLineAndHidesAZdrId(t *testing.T) {
	buf := captureLog(t)
	for _, zdr := range []bool{false, true} {
		srv := testServerFor(grant.NewMemoryRepository(), userPrincipal(zdr))
		resp := acquire(t, srv, &AcquireGrantRequest{})
		buf.Reset()
		if _, err := srv.RevokeGrant(context.Background(), &RevokeGrantRequest{GrantId: resp.GetGrantId()}); err != nil {
			t.Fatal(err)
		}
		out := buf.String()
		if n := strings.Count(out, `"browser grant revoked"`); n != 1 {
			t.Fatalf("zdr=%v: want one revoke line, got %d: %s", zdr, n, out)
		}
		if has := strings.Contains(out, resp.GetGrantId()); has == zdr {
			t.Fatalf("zdr=%v: grant id present in log = %v: %s", zdr, has, out)
		}
	}
}
