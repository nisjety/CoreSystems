package server

// WS21-S4: a non-ZDR read-only grant is issued only with the durable read-only
// switch on (Options.DurableReadOnly). These tests run on the in-memory store;
// the Postgres column is covered by internal/grant/postgres_read_only_integration_test.go.

import (
	"context"
	"strings"
	"testing"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

func TestDurableReadOnly_SwitchOffStillRefusesANonZdrReadOnlyGrant(t *testing.T) {
	srv := testServerFor(grant.NewMemoryRepository(), userPrincipal(false)).WithOptions(Options{DurableReadOnly: false})
	_, err := issue(srv, &AcquireGrantRequest{AllowedDomains: []string{"example.com"}, ReadOnly: true})
	if status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("switch off: code = %v, want FailedPrecondition", status.Code(err))
	}
}

func TestDurableReadOnly_SwitchOnIssuesANonZdrReadOnlyGrant(t *testing.T) {
	srv := testServerFor(grant.NewMemoryRepository(), userPrincipal(false)).WithOptions(Options{DurableReadOnly: true})
	resp, err := issue(srv, &AcquireGrantRequest{AllowedDomains: []string{"example.com"}, ReadOnly: true})
	if err != nil {
		t.Fatalf("switch on: AcquireGrant: %v", err)
	}
	if !resp.GetReadOnly() || resp.GetZdr() || strings.HasPrefix(resp.GetGrantId(), "z_") {
		t.Fatalf("switch on: response = %#v, want a durable read-only grant", resp)
	}
	validated, err := srv.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: resp.GetGrantId()})
	if err != nil {
		t.Fatalf("ValidateGrant: %v", err)
	}
	if !validated.GetReadOnly() {
		t.Fatal("ValidateGrant lost the read level of a durable read-only grant")
	}
}

// A ZDR read-only grant is unchanged by the switch: it was always issued, in
// memory, with the level.
func TestDurableReadOnly_ZdrReadOnlyGrantIsUnchangedBySwitch(t *testing.T) {
	for _, durable := range []bool{false, true} {
		srv := testServerFor(grant.NewMemoryRepository(), userPrincipal(true)).WithOptions(Options{DurableReadOnly: durable})
		resp, err := issue(srv, &AcquireGrantRequest{AllowedDomains: []string{"example.com"}, ReadOnly: true})
		if err != nil {
			t.Fatalf("switch %v: zdr read-only AcquireGrant: %v", durable, err)
		}
		if !strings.HasPrefix(resp.GetGrantId(), "z_") || !resp.GetZdr() || !resp.GetReadOnly() {
			t.Fatalf("switch %v: zdr read-only response = %#v, want an in-memory ZDR read-only grant", durable, resp)
		}
	}
}

// A run derived from a durable read-only grant is read-only (the switch on).
func TestDurableReadOnly_DerivedRunStaysReadOnly(t *testing.T) {
	srv := testServerFor(grant.NewMemoryRepository(), userPrincipal(true)).WithOptions(Options{DurableReadOnly: true})
	parent, err := issue(srv, &AcquireGrantRequest{AllowedDomains: []string{"example.com"}, ReadOnly: true})
	if err != nil {
		t.Fatalf("AcquireGrant: %v", err)
	}
	run, err := srv.DeriveRunGrant(context.Background(), &DeriveRunGrantRequest{
		ParentGrantId: parent.GetGrantId(), SessionKey: "run1", OrgId: "org1", Purpose: derivePurposeAIRun,
	})
	if err != nil {
		t.Fatalf("DeriveRunGrant: %v", err)
	}
	validated, err := srv.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: run.GetGrantId()})
	if err != nil || !validated.GetReadOnly() {
		t.Fatalf("a run derived from a read-only grant: read-only=%v, err=%v", validated.GetReadOnly(), err)
	}
}

// The switch never reaches a sensitive approval: a read-only approval is refused
// whether it is on or off.
func TestDurableReadOnly_SwitchOnStillRefusesAReadOnlyApproval(t *testing.T) {
	srv := testServerFor(grant.NewMemoryRepository(), userPrincipal(true)).WithOptions(Options{DurableReadOnly: true})
	_, err := issue(srv, &AcquireGrantRequest{
		AllowedDomains: []string{"example.com"}, ReadOnly: true,
		AllowedActions: []string{"respond_dialog"}, ParentGrantId: "parent",
	})
	if status.Code(err) != codes.InvalidArgument {
		t.Fatalf("code = %v, want InvalidArgument", status.Code(err))
	}
}

// human_browse has no read level: a read-only ask is refused with the switch on.
func TestDurableReadOnly_SwitchOnStillRefusesReadOnlyHumanBrowse(t *testing.T) {
	cfg := DefaultConfig()
	cfg.HumanBrowseEnabled = true
	srv := NewServerWithConfig(grant.NewMemoryRepository(), cfg)
	srv.principal = func(context.Context) (authctx.Principal, error) { return userPrincipal(false), nil }
	srv.WithOptions(Options{DurableReadOnly: true})
	_, err := srv.AcquireGrant(context.Background(), humanReq(func(r *AcquireGrantRequest) { r.ReadOnly = true }))
	if status.Code(err) != codes.InvalidArgument {
		t.Fatalf("human_browse read-only: code = %v, want InvalidArgument", status.Code(err))
	}
}
