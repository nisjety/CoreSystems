package users

import (
	"context"
	"errors"
	"sync"
	"testing"
)

// fakeOrgCoreSuccessionClient records every promotion call so tests can assert
// EnsureSuccession only calls org-core for the orgs it actually needs to hand
// off, with the correct (orgID, successorID, role) triple.
type fakeOrgCoreSuccessionClient struct {
	mu    sync.Mutex
	calls []fakeSuccessionCall
	err   error
}

type fakeSuccessionCall struct {
	OrgID       string
	SuccessorID string
	Role        string
}

func (f *fakeOrgCoreSuccessionClient) PromoteMemberSuccession(_ context.Context, orgID, successorID, role string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.calls = append(f.calls, fakeSuccessionCall{OrgID: orgID, SuccessorID: successorID, Role: role})
	return f.err
}

func (f *fakeOrgCoreSuccessionClient) callCount() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return len(f.calls)
}

// seedMembership inserts a user_org_memberships row directly, bypassing any
// service-layer validation, so tests can set up exact org topologies.
func seedMembership(t *testing.T, repo *Repository, id, userID, orgID, role, status string) {
	t.Helper()
	if _, err := repo.db.Pool.Exec(context.Background(), `
INSERT INTO user_org_memberships (id, user_id, org_id, role, status)
VALUES ($1, $2, $3, $4, $5)`, id, userID, orgID, role, status); err != nil {
		t.Fatalf("seed membership %s: %v", id, err)
	}
}

// TestSoleAdminOrgsIdentifiesOnlyOrganizationsWithoutAnotherOwnerOrAdmin
// covers the exact query EnsureSuccession's gate depends on.
func TestSoleAdminOrgsIdentifiesOnlyOrganizationsWithoutAnotherOwnerOrAdmin(t *testing.T) {
	_, repo := newErasurePostgresFixture(t)

	// org-sole: u-sole is the ONLY active owner.
	seedMembership(t, repo, "m-sole-owner", "u-sole", "org-sole", "owner", "active")
	// org-shared: u-sole is owner, but u-other is ALSO an active admin.
	seedMembership(t, repo, "m-shared-owner", "u-sole", "org-shared", "owner", "active")
	seedMembership(t, repo, "m-shared-admin", "u-other", "org-shared", "admin", "active")
	// org-removed-peer: the second owner is REMOVED, so u-sole is effectively sole again.
	seedMembership(t, repo, "m-removed-peer-owner", "u-sole", "org-removed-peer", "owner", "active")
	seedMembership(t, repo, "m-removed-peer-second", "u-second", "org-removed-peer", "owner", "removed")
	// org-member-only: u-sole is a plain member, not owner/admin — must be excluded.
	seedMembership(t, repo, "m-member-only", "u-sole", "org-member-only", "member", "active")

	orgs, err := repo.SoleAdminOrgs(context.Background(), "u-sole")
	if err != nil {
		t.Fatalf("SoleAdminOrgs: %v", err)
	}
	var ids []string
	for _, o := range orgs {
		ids = append(ids, o.OrgID)
	}
	want := []string{"org-removed-peer", "org-sole"}
	if len(ids) != len(want) || ids[0] != want[0] || ids[1] != want[1] {
		t.Fatalf("SoleAdminOrgs(u-sole) = %v, want %v", ids, want)
	}

	if orgs, err := repo.SoleAdminOrgs(context.Background(), "u-other"); err != nil || len(orgs) != 0 {
		t.Fatalf("SoleAdminOrgs(u-other) = %v err=%v, want empty (co-admin of org-shared only)", orgs, err)
	}
	if orgs, err := repo.SoleAdminOrgs(context.Background(), "u-nobody"); err != nil || len(orgs) != 0 {
		t.Fatalf("SoleAdminOrgs(u-nobody) = %v err=%v, want empty", orgs, err)
	}
}

// TestEnsureSuccessionAllowsErasureWhenNotSoleAdmin proves the common case
// (no sole-admin org at all) never touches org-core.
func TestEnsureSuccessionAllowsErasureWhenNotSoleAdmin(t *testing.T) {
	_, repo := newErasurePostgresFixture(t)
	seedMembership(t, repo, "m-1", "u-multi", "org-1", "owner", "active")
	seedMembership(t, repo, "m-2", "u-peer", "org-1", "admin", "active")

	svc := NewService(repo, nil, nil)
	client := &fakeOrgCoreSuccessionClient{}
	svc.SetOrgCoreClient(client)

	if err := svc.EnsureSuccession(context.Background(), "u-multi", ""); err != nil {
		t.Fatalf("EnsureSuccession returned error for non-sole-admin user: %v", err)
	}
	if n := client.callCount(); n != 0 {
		t.Fatalf("EnsureSuccession called org-core %d times for a user who needed no successor", n)
	}
}

// TestEnsureSuccessionRequiresSuccessorForSoleAdmin proves the 409-shaped gate
// fires with no org-core call when no successor was supplied.
func TestEnsureSuccessionRequiresSuccessorForSoleAdmin(t *testing.T) {
	_, repo := newErasurePostgresFixture(t)
	seedMembership(t, repo, "m-1", "u-sole", "org-sole", "owner", "active")

	svc := NewService(repo, nil, nil)
	client := &fakeOrgCoreSuccessionClient{}
	svc.SetOrgCoreClient(client)

	err := svc.EnsureSuccession(context.Background(), "u-sole", "")
	var required *ErrSuccessorRequired
	if !errors.As(err, &required) {
		t.Fatalf("EnsureSuccession error = %v, want *ErrSuccessorRequired", err)
	}
	if len(required.Orgs) != 1 || required.Orgs[0].OrgID != "org-sole" {
		t.Fatalf("ErrSuccessorRequired.Orgs = %v, want [org-sole]", required.Orgs)
	}
	if n := client.callCount(); n != 0 {
		t.Fatalf("EnsureSuccession called org-core %d times before a successor was supplied", n)
	}
}

// TestEnsureSuccessionRejectsInvalidSuccessors covers both invalid-successor
// shapes: not an active member, and the departing user themselves.
func TestEnsureSuccessionRejectsInvalidSuccessors(t *testing.T) {
	_, repo := newErasurePostgresFixture(t)
	seedMembership(t, repo, "m-1", "u-sole", "org-sole", "owner", "active")
	seedMembership(t, repo, "m-2", "u-removed", "org-sole", "member", "removed")

	svc := NewService(repo, nil, nil)
	client := &fakeOrgCoreSuccessionClient{}
	svc.SetOrgCoreClient(client)

	err := svc.EnsureSuccession(context.Background(), "u-sole", "u-sole")
	var invalidSelf *ErrSuccessorInvalid
	if !errors.As(err, &invalidSelf) {
		t.Fatalf("EnsureSuccession(self as successor) error = %v, want *ErrSuccessorInvalid", err)
	}

	err = svc.EnsureSuccession(context.Background(), "u-sole", "u-not-a-member")
	var invalidStranger *ErrSuccessorInvalid
	if !errors.As(err, &invalidStranger) {
		t.Fatalf("EnsureSuccession(non-member successor) error = %v, want *ErrSuccessorInvalid", err)
	}

	err = svc.EnsureSuccession(context.Background(), "u-sole", "u-removed")
	var invalidRemoved *ErrSuccessorInvalid
	if !errors.As(err, &invalidRemoved) {
		t.Fatalf("EnsureSuccession(removed-member successor) error = %v, want *ErrSuccessorInvalid", err)
	}

	if n := client.callCount(); n != 0 {
		t.Fatalf("EnsureSuccession called org-core %d times for invalid successors", n)
	}
}

// TestEnsureSuccessionFailsClosedWithoutOrgCoreClient proves a deployment that
// hasn't configured the org-core succession client refuses (rather than
// silently skipping) the handoff for a user who actually needs one.
func TestEnsureSuccessionFailsClosedWithoutOrgCoreClient(t *testing.T) {
	_, repo := newErasurePostgresFixture(t)
	seedMembership(t, repo, "m-1", "u-sole", "org-sole", "owner", "active")
	seedMembership(t, repo, "m-2", "u-successor", "org-sole", "member", "active")

	svc := NewService(repo, nil, nil) // SetOrgCoreClient never called

	err := svc.EnsureSuccession(context.Background(), "u-sole", "u-successor")
	if err == nil {
		t.Fatal("EnsureSuccession succeeded with no org-core client configured")
	}
	var required *ErrSuccessorRequired
	var invalid *ErrSuccessorInvalid
	if errors.As(err, &required) || errors.As(err, &invalid) {
		t.Fatalf("expected a plain configuration error, got structured error %v", err)
	}
}

// TestEnsureSuccessionPromotesValidSuccessorAndCoversMultipleSoleAdminOrgs is
// the full happy path, including the rare multi-org edge case: the SAME
// successor must be handed off in every sole-admin org.
func TestEnsureSuccessionPromotesValidSuccessorAndCoversMultipleSoleAdminOrgs(t *testing.T) {
	_, repo := newErasurePostgresFixture(t)
	seedMembership(t, repo, "m-1", "u-sole", "org-a", "owner", "active")
	seedMembership(t, repo, "m-2", "u-successor", "org-a", "member", "active")
	seedMembership(t, repo, "m-3", "u-sole", "org-b", "admin", "active")
	seedMembership(t, repo, "m-4", "u-successor", "org-b", "member", "active")

	svc := NewService(repo, nil, nil)
	client := &fakeOrgCoreSuccessionClient{}
	svc.SetOrgCoreClient(client)

	if err := svc.EnsureSuccession(context.Background(), "u-sole", "u-successor"); err != nil {
		t.Fatalf("EnsureSuccession(valid successor, 2 sole-admin orgs): %v", err)
	}
	if n := client.callCount(); n != 2 {
		t.Fatalf("org-core call count = %d, want 2", n)
	}
	byOrg := map[string]fakeSuccessionCall{}
	for _, call := range client.calls {
		byOrg[call.OrgID] = call
	}
	if call, ok := byOrg["org-a"]; !ok || call.SuccessorID != "u-successor" || call.Role != "owner" {
		t.Fatalf("org-a promotion call = %+v, want successor=u-successor role=owner", call)
	}
	if call, ok := byOrg["org-b"]; !ok || call.SuccessorID != "u-successor" || call.Role != "admin" {
		t.Fatalf("org-b promotion call = %+v, want successor=u-successor role=admin", call)
	}
}

// TestEnsureSuccessionPropagatesOrgCoreFailure proves a failed promotion
// aborts EnsureSuccession (and, by construction, the caller's erasure) rather
// than proceeding with a half-completed handoff.
func TestEnsureSuccessionPropagatesOrgCoreFailure(t *testing.T) {
	_, repo := newErasurePostgresFixture(t)
	seedMembership(t, repo, "m-1", "u-sole", "org-sole", "owner", "active")
	seedMembership(t, repo, "m-2", "u-successor", "org-sole", "member", "active")

	svc := NewService(repo, nil, nil)
	client := &fakeOrgCoreSuccessionClient{err: errors.New("org-core unreachable")}
	svc.SetOrgCoreClient(client)

	err := svc.EnsureSuccession(context.Background(), "u-sole", "u-successor")
	if err == nil {
		t.Fatal("EnsureSuccession succeeded despite org-core promotion failure")
	}
	if client.callCount() != 1 {
		t.Fatalf("org-core call count = %d, want 1", client.callCount())
	}
}
