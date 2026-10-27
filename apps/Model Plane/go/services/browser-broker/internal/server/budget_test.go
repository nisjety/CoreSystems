package server

import (
	"context"
	"testing"

	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

func budgetOf(req interface {
	GetBudgetMicroUsd() int64
	GetBudgetMaxSteps() int32
	GetBudgetMaxRuntimeS() int32
	GetBudgetSource() string
}) [4]any {
	return [4]any{req.GetBudgetMicroUsd(), req.GetBudgetMaxSteps(), req.GetBudgetMaxRuntimeS(), req.GetBudgetSource()}
}

func TestAcquireGrantStoresAndReturnsABudgetClaim(t *testing.T) {
	srv := testServerFor(grant.NewMemoryRepository(), userPrincipal(false))
	resp := acquire(t, srv, &AcquireGrantRequest{
		BudgetMicroUsd: 2_000_000, BudgetMaxSteps: 30, BudgetMaxRuntimeS: 600, BudgetSource: "org_default",
	})
	if resp.GetBudgetMicroUsd() != 2_000_000 || resp.GetBudgetMaxSteps() != 30 || resp.GetBudgetMaxRuntimeS() != 600 ||
		resp.GetBudgetSource() != "org_default" || resp.GetBudgetMintedAtUnix() == 0 {
		t.Fatalf("acquire response claim = %v minted=%d", budgetOf(resp), resp.GetBudgetMintedAtUnix())
	}
	// A grant minted without a claim carries none (the old behaviour).
	bare := acquire(t, srv, &AcquireGrantRequest{SessionKey: "bare"})
	if bare.GetBudgetMicroUsd() != 0 || bare.GetBudgetMaxSteps() != 0 || bare.GetBudgetMaxRuntimeS() != 0 ||
		bare.GetBudgetSource() != "" || bare.GetBudgetMintedAtUnix() != 0 {
		t.Fatalf("an unclaimed grant carries %v", budgetOf(bare))
	}
}

// A run grant says which run it was derived for (D1-001): ValidateGrant returns the
// session key it was issued under, so a consumer can bind a presented run grant to
// that one run. The parent session grant returns its own key.
func TestValidateGrantReturnsTheSessionKeyTheRunGrantWasDerivedUnder(t *testing.T) {
	srv := testServerFor(grant.NewMemoryRepository(), userPrincipal(false))
	parent := acquire(t, srv, &AcquireGrantRequest{})
	run, err := srv.DeriveRunGrant(context.Background(), deriveReq(parent.GetGrantId(), func(r *DeriveRunGrantRequest) {
		r.SessionKey = "browser-run-key-1"
	}))
	if err != nil {
		t.Fatal(err)
	}
	got, err := srv.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: run.GetGrantId()})
	if err != nil {
		t.Fatal(err)
	}
	if got.GetKind() != "run" || got.GetSessionKey() != "browser-run-key-1" {
		t.Fatalf("run grant: kind=%q session_key=%q", got.GetKind(), got.GetSessionKey())
	}
	p, err := srv.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: parent.GetGrantId()})
	if err != nil {
		t.Fatal(err)
	}
	if p.GetKind() != "session" || p.GetSessionKey() == "" || p.GetSessionKey() == got.GetSessionKey() {
		t.Fatalf("session grant: kind=%q session_key=%q", p.GetKind(), p.GetSessionKey())
	}
}

func TestValidateGrantReturnsBudgetVerbatim(t *testing.T) {
	srv := testServerFor(grant.NewMemoryRepository(), userPrincipal(false))
	parent := acquire(t, srv, &AcquireGrantRequest{
		BudgetMicroUsd: 5_000_000, BudgetMaxSteps: 20, BudgetSource: "entitlement",
	})
	run, err := srv.DeriveRunGrant(context.Background(), deriveReq(parent.GetGrantId(), func(r *DeriveRunGrantRequest) {
		r.BudgetMicroUsd = 1_500_000
		r.BudgetSource = "request"
	}))
	if err != nil {
		t.Fatal(err)
	}
	got, err := srv.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: run.GetGrantId()})
	if err != nil {
		t.Fatal(err)
	}
	// What validate says is what the derive response said, and what was stored.
	if got.GetBudgetMicroUsd() != 1_500_000 || got.GetBudgetMaxSteps() != 20 || got.GetBudgetMaxRuntimeS() != 0 ||
		got.GetBudgetSource() != "request" || got.GetBudgetMintedAtUnix() == 0 {
		t.Fatalf("validate claim = %v minted=%d", budgetOf(got), got.GetBudgetMintedAtUnix())
	}
	if budgetOf(got) != budgetOf(run) || got.GetBudgetMintedAtUnix() != run.GetBudgetMintedAtUnix() {
		t.Fatalf("validate %v differs from derive %v", budgetOf(got), budgetOf(run))
	}
	// The parent's own claim is returned too, unchanged by its child.
	p, err := srv.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: parent.GetGrantId()})
	if err != nil || p.GetBudgetMicroUsd() != 5_000_000 || p.GetBudgetSource() != "entitlement" {
		t.Fatalf("parent after derive: %v, %v", p, err)
	}
}

func TestDeriveRunGrantBudgetNeverWidensParent(t *testing.T) {
	srv := testServerFor(grant.NewMemoryRepository(), userPrincipal(false))
	parent := acquire(t, srv, &AcquireGrantRequest{
		BudgetMicroUsd: 5_000_000, BudgetMaxSteps: 20, BudgetMaxRuntimeS: 600, BudgetSource: "org_default",
	})
	for name, mutate := range map[string]func(*DeriveRunGrantRequest){
		"cost":    func(r *DeriveRunGrantRequest) { r.BudgetMicroUsd = 5_000_001; r.BudgetSource = "request" },
		"steps":   func(r *DeriveRunGrantRequest) { r.BudgetMaxSteps = 21; r.BudgetSource = "request" },
		"runtime": func(r *DeriveRunGrantRequest) { r.BudgetMaxRuntimeS = 601; r.BudgetSource = "request" },
	} {
		t.Run(name, func(t *testing.T) {
			_, err := srv.DeriveRunGrant(context.Background(), deriveReq(parent.GetGrantId(), mutate))
			if status.Code(err) != codes.PermissionDenied {
				t.Fatalf("code = %v (%v), want PermissionDenied", status.Code(err), err)
			}
		})
	}
	// A request with no claim cannot drop the parent's: it inherits it.
	run, err := srv.DeriveRunGrant(context.Background(), deriveReq(parent.GetGrantId()))
	if err != nil {
		t.Fatal(err)
	}
	if run.GetBudgetMicroUsd() != 5_000_000 || run.GetBudgetMaxSteps() != 20 || run.GetBudgetMaxRuntimeS() != 600 {
		t.Fatalf("inherited claim = %v", budgetOf(run))
	}
}

func TestBudgetClaimShapeIsValidatedAtTheBoundary(t *testing.T) {
	srv := testServerFor(grant.NewMemoryRepository(), userPrincipal(false))
	parent := acquire(t, srv, &AcquireGrantRequest{})
	bad := map[string]func(budget *claimFields){
		"negative cost":    func(c *claimFields) { c.micro = -1; c.source = "request" },
		"negative steps":   func(c *claimFields) { c.steps = -1; c.source = "request" },
		"negative runtime": func(c *claimFields) { c.runtime = -1; c.source = "request" },
		"unknown source":   func(c *claimFields) { c.micro = 1; c.source = "client" },
		"bound, no source": func(c *claimFields) { c.micro = 1 },
		"source, no bound": func(c *claimFields) { c.source = "request" },
		"caller-set stamp": func(c *claimFields) { c.micro = 1; c.source = "request"; c.minted = 1_700_000_000 },
		"stamp alone":      func(c *claimFields) { c.minted = 1_700_000_000 },
	}
	for name, mutate := range bad {
		t.Run("acquire/"+name, func(t *testing.T) {
			var c claimFields
			mutate(&c)
			_, err := srv.AcquireGrant(context.Background(), &AcquireGrantRequest{
				SessionKey: "k-" + name, OrgId: "org1", AllowedDomains: []string{"example.com"},
				BudgetMicroUsd: c.micro, BudgetMaxSteps: c.steps, BudgetMaxRuntimeS: c.runtime,
				BudgetSource: c.source, BudgetMintedAtUnix: c.minted,
			})
			if status.Code(err) != codes.InvalidArgument {
				t.Fatalf("code = %v (%v), want InvalidArgument", status.Code(err), err)
			}
		})
		t.Run("derive/"+name, func(t *testing.T) {
			var c claimFields
			mutate(&c)
			_, err := srv.DeriveRunGrant(context.Background(), deriveReq(parent.GetGrantId(), func(r *DeriveRunGrantRequest) {
				r.SessionKey = "d-" + name
				r.BudgetMicroUsd, r.BudgetMaxSteps, r.BudgetMaxRuntimeS = c.micro, c.steps, c.runtime
				r.BudgetSource, r.BudgetMintedAtUnix = c.source, c.minted
			}))
			if status.Code(err) != codes.InvalidArgument {
				t.Fatalf("code = %v (%v), want InvalidArgument", status.Code(err), err)
			}
		})
	}
}

type claimFields struct {
	micro   int64
	steps   int32
	runtime int32
	source  string
	minted  int64
}

func TestASensitiveApprovalCannotCarryABudgetClaim(t *testing.T) {
	srv := testServerFor(grant.NewMemoryRepository(), userPrincipal(false))
	parent := acquire(t, srv, &AcquireGrantRequest{})
	_, err := srv.AcquireGrant(context.Background(), &AcquireGrantRequest{
		SessionKey:      "session-a",
		OrgId:           "org1",
		AllowedDomains:  []string{"example.com"},
		AllowedActions:  []string{"download_ref"},
		AllowedFrameIds: []string{"frame-1"},
		ParentGrantId:   parent.GetGrantId(),
		BudgetMaxSteps:  3,
		BudgetSource:    "request",
	})
	if status.Code(err) != codes.InvalidArgument {
		t.Fatalf("code = %v (%v), want InvalidArgument", status.Code(err), err)
	}
}
