package server

import (
	"context"
	"testing"
	"time"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
)

// approvalTestTTL is the configured grant lifetime for these tests. It is not
// the 15-minute default, so a pass cannot come from the default being in place.
const approvalTestTTL = 2 * time.Minute

// approvalTTLSlack absorbs the gap between the test reading the clock and the
// grant layer reading it, plus the clock's own granularity.
const approvalTTLSlack = 5 * time.Second

// approvalTTLServer is a server whose configured TTL is approvalTestTTL, backed
// by store, so an approval can find the parent it was issued under.
func approvalTTLServer(store grant.Repository) *Server {
	cfg := DefaultConfig()
	cfg.TTL = approvalTestTTL
	srv := NewServerWithConfig(store, cfg)
	srv.principal = func(context.Context) (authctx.Principal, error) { return userPrincipal(false), nil }
	return srv
}

// approvalRequest is a sensitive approval under parent. Only ttlSeconds varies.
func approvalRequest(parent *AcquireGrantResponse, ttlSeconds int32) *AcquireGrantRequest {
	return &AcquireGrantRequest{
		AllowedActions:  []string{"download_ref"},
		AllowedFrameIds: []string{"frame-1"},
		ParentGrantId:   parent.GetGrantId(),
		TtlSeconds:      ttlSeconds,
	}
}

// D0-012: a sensitive approval takes the configured TTL, not the ttl_seconds the
// caller sent. The parent outlives the configured TTL, so the cap at the parent's
// expiry cannot be what sets the approval's lifetime; without the fix the
// approval would last the 30 minutes asked for.
func TestSensitiveApprovalIgnoresTheRequestedTTL(t *testing.T) {
	srv := approvalTTLServer(grant.NewMemoryRepository())
	parent := acquire(t, srv, &AcquireGrantRequest{TtlSeconds: 1800})

	before := time.Now()
	child := acquire(t, srv, approvalRequest(parent, 1800))
	lifetime := child.GetExpiresAt().AsTime().Sub(before)
	if lifetime < approvalTestTTL-approvalTTLSlack || lifetime > approvalTestTTL+approvalTTLSlack {
		t.Fatalf("approval lifetime = %v, want the configured %v", lifetime, approvalTestTTL)
	}
}

// ttl_seconds is read only for an ordinary grant. A sensitive approval never
// reads it, so a value below the ordinary-grant minimum must not refuse one.
func TestSensitiveApprovalIsNotRefusedForASubMinimumTTL(t *testing.T) {
	srv := approvalTTLServer(grant.NewMemoryRepository())
	parent := acquire(t, srv, &AcquireGrantRequest{TtlSeconds: 1800})

	before := time.Now()
	child := acquire(t, srv, approvalRequest(parent, 10))
	if child.GetKind() != "sensitive_approval" {
		t.Fatalf("kind = %q, want sensitive_approval", child.GetKind())
	}
	lifetime := child.GetExpiresAt().AsTime().Sub(before)
	if lifetime < approvalTestTTL-approvalTTLSlack || lifetime > approvalTestTTL+approvalTTLSlack {
		t.Fatalf("approval lifetime = %v, want the configured %v", lifetime, approvalTestTTL)
	}
}

// A grant with no allowed_actions still honours a valid ttl_seconds: the field
// is ignored only on the sensitive path. The configured TTL is 2 minutes, so a
// 10-minute lifetime can only have come from the request.
func TestRunGrantHonoursAValidRequestedTTL(t *testing.T) {
	srv := approvalTTLServer(grant.NewMemoryRepository())

	before := time.Now()
	run := acquire(t, srv, &AcquireGrantRequest{TtlSeconds: 600})
	const want = 10 * time.Minute
	lifetime := run.GetExpiresAt().AsTime().Sub(before)
	if lifetime < want-approvalTTLSlack || lifetime > want+approvalTTLSlack {
		t.Fatalf("grant lifetime = %v, want about %v from the request", lifetime, want)
	}
}
