package org

import (
	"context"
	"errors"
	"fmt"
	"os"
	"sync"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
)

// WS05-S8: SCIM tokens, SCIM users and the fake-provisioner flows against a real Postgres.
// Gated like the other identity tests: they skip unless CONTROL_LIFECYCLE_TEST_DATABASE_URL names a
// disposable, marked database. Deferred to the DB-harness tier.

type fakeScimProvisioner struct {
	mu            sync.Mutex
	provisioned   []SCIMProvisionRequest
	deprovisioned []SCIMDeprovisionRequest
	userIDs       map[string]string // email -> user id
	deprovRole    string            // role auth-core reports on deprovision
	deprovErr     error
	provisionErr  error
}

func newFakeScimProvisioner() *fakeScimProvisioner {
	return &fakeScimProvisioner{userIDs: map[string]string{}, deprovRole: "member"}
}

func (f *fakeScimProvisioner) ProvisionMember(_ context.Context, req SCIMProvisionRequest) (*SCIMProvisionResult, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.provisioned = append(f.provisioned, req)
	if f.provisionErr != nil {
		return nil, f.provisionErr
	}
	id, ok := f.userIDs[req.Email]
	if !ok {
		id = fmt.Sprintf("user-%d", len(f.userIDs)+1)
		f.userIDs[req.Email] = id
	}
	return &SCIMProvisionResult{UserID: id, MemberID: "member-" + id, Role: req.Role, MemberCreated: true}, nil
}

func (f *fakeScimProvisioner) DeprovisionMember(_ context.Context, req SCIMDeprovisionRequest) (*SCIMDeprovisionResult, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.deprovisioned = append(f.deprovisioned, req)
	if f.deprovErr != nil {
		return nil, f.deprovErr
	}
	return &SCIMDeprovisionResult{Removed: true, Role: f.deprovRole}, nil
}

type scimHarness struct {
	*identityHarness
	prov *fakeScimProvisioner
}

func newScimHarness(t *testing.T) *scimHarness {
	t.Helper()
	h := newIdentityHarness(t)
	prov := newFakeScimProvisioner()
	h.service.SetScimProvisioner(prov)
	return &scimHarness{identityHarness: h, prov: prov}
}

// orgWithDomain seeds an organisation holding one verified domain.
func (h *scimHarness) orgWithDomain(t *testing.T, label, domain string) string {
	t.Helper()
	org := h.newOrg(t, label)
	h.addAndVerify(t, org, domain)
	return org
}

// authFor mints a token and resolves it, the way a SCIM request does.
func (h *scimHarness) authFor(t *testing.T, org string) *SCIMTokenAuth {
	t.Helper()
	issued, err := h.service.CreateSCIMToken(h.ctx, org, "admin-1", "admin", "idp", 30)
	if err != nil {
		t.Fatalf("create token: %v", err)
	}
	auth, err := h.service.ResolveSCIMToken(h.ctx, issued.BearerToken)
	if err != nil {
		t.Fatalf("resolve token: %v", err)
	}
	return auth
}

func (h *scimHarness) activate(active bool) *bool { return &active }

// --- tokens ---

func TestSCIMTokenNeedsAVerifiedDomain(t *testing.T) {
	h := newScimHarness(t)
	org := h.newOrg(t, "scim-nodomain")
	if _, err := h.service.CreateSCIMToken(h.ctx, org, "admin-1", "admin", "idp", 30); !errors.Is(err, ErrSCIMNoVerifiedDomain) {
		t.Fatalf("token without a verified domain = %v, want ErrSCIMNoVerifiedDomain", err)
	}
	// A pending (unproven) domain does not count.
	if _, err := h.service.AddIdentityDomain(h.ctx, org, "admin-1", "admin", "pending.example.com"); err != nil {
		t.Fatal(err)
	}
	if _, err := h.service.CreateSCIMToken(h.ctx, org, "admin-1", "admin", "idp", 30); !errors.Is(err, ErrSCIMNoVerifiedDomain) {
		t.Fatalf("token with only a pending domain = %v", err)
	}
	if n := h.outboxCount(t, org, IdentityEventSCIMTokenCreated); n != 0 {
		t.Fatalf("a refused token left %d audit rows", n)
	}
}

func TestSCIMTokenLimitIsThreeActiveAndRevokedOrExpiredDoNotCount(t *testing.T) {
	h := newScimHarness(t)
	org := h.orgWithDomain(t, "scim-limit", "limit.example.com")

	var issued []*SCIMTokenIssued
	for i := 0; i < MaxActiveSCIMTokens; i++ {
		tok, err := h.service.CreateSCIMToken(h.ctx, org, "admin-1", "admin", fmt.Sprintf("idp-%d", i), 30)
		if err != nil {
			t.Fatalf("token %d: %v", i, err)
		}
		issued = append(issued, tok)
	}
	if _, err := h.service.CreateSCIMToken(h.ctx, org, "admin-1", "admin", "one-too-many", 30); !errors.Is(err, ErrSCIMTokenLimit) {
		t.Fatalf("4th token = %v, want ErrSCIMTokenLimit", err)
	}
	if err := h.service.RevokeSCIMToken(h.ctx, org, "admin-1", "admin", issued[0].Token.ID); err != nil {
		t.Fatal(err)
	}
	if _, err := h.service.CreateSCIMToken(h.ctx, org, "admin-1", "admin", "after-revoke", 30); err != nil {
		t.Fatalf("a revoked token must free its slot: %v", err)
	}
	if _, err := h.db.Pool.Exec(h.ctx, `UPDATE organization_scim_tokens SET expires_at = NOW() - INTERVAL '1 hour' WHERE id = $1`, issued[1].Token.ID); err != nil {
		t.Fatal(err)
	}
	if _, err := h.service.CreateSCIMToken(h.ctx, org, "admin-1", "admin", "after-expiry", 30); err != nil {
		t.Fatalf("an expired token must free its slot: %v", err)
	}

	// The audit row is written in the same transaction as the insert: 5 creates, 1 revoke.
	if n := h.outboxCount(t, org, IdentityEventSCIMTokenCreated); n != 5 {
		t.Fatalf("created audit rows = %d, want 5", n)
	}
	if n := h.outboxCount(t, org, IdentityEventSCIMTokenRevoked); n != 1 {
		t.Fatalf("revoked audit rows = %d, want 1", n)
	}
}

func TestSCIMTokenListShowsPrefixButNeverTheSecretOrHash(t *testing.T) {
	h := newScimHarness(t)
	org := h.orgWithDomain(t, "scim-list", "list.example.com")
	issued, err := h.service.CreateSCIMToken(h.ctx, org, "admin-1", "admin", "Entra prod", 7)
	if err != nil {
		t.Fatal(err)
	}
	if issued.BearerToken[:len(issued.Token.Prefix)] != issued.Token.Prefix {
		t.Fatalf("prefix %q is not the start of the bearer token", issued.Token.Prefix)
	}
	listed, err := h.service.ListSCIMTokens(h.ctx, org)
	if err != nil || len(listed) != 1 {
		t.Fatalf("list: %+v, %v", listed, err)
	}
	if listed[0].ID != issued.Token.ID || listed[0].Name != "Entra prod" || listed[0].Prefix != issued.Token.Prefix {
		t.Fatalf("listed token: %+v", listed[0])
	}
	want := issued.Token.CreatedAt.Add(7 * 24 * time.Hour)
	if d := listed[0].ExpiresAt.Sub(want); d > time.Second || d < -time.Second {
		t.Fatalf("expires_at %v, want about %v", listed[0].ExpiresAt, want)
	}
}

func TestSCIMTokenResolveByHashAndUnauthorizedForRevokedExpiredAndDeletedOrg(t *testing.T) {
	h := newScimHarness(t)
	org := h.orgWithDomain(t, "scim-resolve", "resolve.example.com")

	good, err := h.service.CreateSCIMToken(h.ctx, org, "admin-1", "admin", "good", 30)
	if err != nil {
		t.Fatal(err)
	}
	auth, err := h.service.ResolveSCIMToken(h.ctx, good.BearerToken)
	if err != nil || auth.OrgID != org || auth.TokenID != good.Token.ID || auth.CreatedBy != "admin-1" {
		t.Fatalf("resolve: %+v, %v", auth, err)
	}
	// The organisation comes from the token row: a one-character change resolves nothing.
	if _, err := h.service.ResolveSCIMToken(h.ctx, good.BearerToken+"x"); !errors.Is(err, ErrSCIMUnauthorized) {
		t.Fatalf("tampered token = %v", err)
	}

	// last_used_at is set by the first use.
	var lastUsed *time.Time
	if err := h.db.Pool.QueryRow(h.ctx, `SELECT last_used_at FROM organization_scim_tokens WHERE id = $1`, good.Token.ID).Scan(&lastUsed); err != nil || lastUsed == nil {
		t.Fatalf("last_used_at after a resolve: %v, %v", lastUsed, err)
	}

	revoked, _ := h.service.CreateSCIMToken(h.ctx, org, "admin-1", "admin", "revoked", 30)
	if err := h.service.RevokeSCIMToken(h.ctx, org, "admin-1", "admin", revoked.Token.ID); err != nil {
		t.Fatal(err)
	}
	if _, err := h.service.ResolveSCIMToken(h.ctx, revoked.BearerToken); !errors.Is(err, ErrSCIMUnauthorized) {
		t.Fatalf("revoked token = %v", err)
	}

	expired, _ := h.service.CreateSCIMToken(h.ctx, org, "admin-1", "admin", "expired", 30)
	if _, err := h.db.Pool.Exec(h.ctx, `UPDATE organization_scim_tokens SET expires_at = NOW() - INTERVAL '1 second' WHERE id = $1`, expired.Token.ID); err != nil {
		t.Fatal(err)
	}
	if _, err := h.service.ResolveSCIMToken(h.ctx, expired.BearerToken); !errors.Is(err, ErrSCIMUnauthorized) {
		t.Fatalf("expired token = %v", err)
	}

	if _, err := h.db.Pool.Exec(h.ctx, `UPDATE organizations SET deleted_at = NOW() WHERE id = $1`, org); err != nil {
		t.Fatal(err)
	}
	if _, err := h.service.ResolveSCIMToken(h.ctx, good.BearerToken); !errors.Is(err, ErrSCIMUnauthorized) {
		t.Fatalf("token of a deleted organisation = %v", err)
	}
}

func TestSCIMTokenRevokeIsIdempotentAndScopedToTheOrganisation(t *testing.T) {
	h := newScimHarness(t)
	orgA := h.orgWithDomain(t, "scim-revoke-a", "revoke-a.example.com")
	orgB := h.orgWithDomain(t, "scim-revoke-b", "revoke-b.example.com")
	tok, err := h.service.CreateSCIMToken(h.ctx, orgA, "admin-1", "admin", "idp", 30)
	if err != nil {
		t.Fatal(err)
	}

	// Another organisation's admin cannot see or revoke it.
	if err := h.service.RevokeSCIMToken(h.ctx, orgB, "admin-2", "admin", tok.Token.ID); !errors.Is(err, ErrSCIMTokenNotFound) {
		t.Fatalf("cross-org revoke = %v, want ErrSCIMTokenNotFound", err)
	}
	if err := h.service.RevokeSCIMToken(h.ctx, orgA, "admin-1", "admin", "11111111-2222-3333-4444-555555555555"); !errors.Is(err, ErrSCIMTokenNotFound) {
		t.Fatalf("unknown token = %v", err)
	}
	if err := h.service.RevokeSCIMToken(h.ctx, orgA, "admin-1", "admin", tok.Token.ID); err != nil {
		t.Fatal(err)
	}
	if err := h.service.RevokeSCIMToken(h.ctx, orgA, "admin-1", "admin", tok.Token.ID); err != nil {
		t.Fatalf("revoking twice = %v, want a no-op", err)
	}
	if n := h.outboxCount(t, orgA, IdentityEventSCIMTokenRevoked); n != 1 {
		t.Fatalf("revoke audit rows = %d, want 1 (the second revoke is a no-op)", n)
	}
}

func TestSCIMStatusCountsTokensDomainsAndUsers(t *testing.T) {
	h := newScimHarness(t)
	org := h.orgWithDomain(t, "scim-status", "status.example.com")
	auth := h.authFor(t, org)
	if _, err := h.service.CreateSCIMUser(h.ctx, auth, SCIMUserInput{UserName: "a@status.example.com"}); err != nil {
		t.Fatal(err)
	}
	off := false
	if _, err := h.service.CreateSCIMUser(h.ctx, auth, SCIMUserInput{UserName: "b@status.example.com", Active: &off}); err != nil {
		t.Fatal(err)
	}
	st, err := h.service.GetSCIMStatus(h.ctx, org)
	if err != nil {
		t.Fatal(err)
	}
	if !st.Enabled || st.VerifiedDomains != 1 || st.ActiveTokens != 1 || st.MaxTokens != 3 || st.ManagedUsers != 2 || st.ActiveUsers != 1 || st.LastUsedAt == nil {
		t.Fatalf("status: %+v", st)
	}
}

// --- SCIM users with the fake provisioner ---

func TestSCIMCreateUserOnAnUnverifiedDomainIsRefused(t *testing.T) {
	h := newScimHarness(t)
	org := h.orgWithDomain(t, "scim-unverified", "verified.example.com")
	auth := h.authFor(t, org)
	_, err := h.service.CreateSCIMUser(h.ctx, auth, SCIMUserInput{UserName: "eve@other.example.com"})
	if !errors.Is(err, ErrSCIMDomainNotVerified) {
		t.Fatalf("create on an unverified domain = %v, want ErrSCIMDomainNotVerified", err)
	}
	if len(h.prov.provisioned) != 0 {
		t.Fatalf("auth-core was asked to provision %d members for a refused request", len(h.prov.provisioned))
	}
}

func TestSCIMCreateUserAlwaysProvisionsAMemberAndNormalisesTheName(t *testing.T) {
	h := newScimHarness(t)
	org := h.orgWithDomain(t, "scim-create", "create.example.com")
	auth := h.authFor(t, org)

	rec, err := h.service.CreateSCIMUser(h.ctx, auth, SCIMUserInput{UserName: "  Ada@Create.Example.com ", ExternalID: " ext-1 "})
	if err != nil {
		t.Fatal(err)
	}
	if rec.UserName != "ada@create.example.com" || rec.ExternalID != "ext-1" || !rec.Active || rec.Revision != 1 || rec.UserID == "" {
		t.Fatalf("record: %+v", rec)
	}
	if len(h.prov.provisioned) != 1 {
		t.Fatalf("provision calls = %d", len(h.prov.provisioned))
	}
	req := h.prov.provisioned[0]
	if req.Role != "member" {
		t.Fatalf("SCIM must never grant more than member, asked for %q", req.Role)
	}
	if req.OrganizationID != org || req.Email != "ada@create.example.com" || req.ActorUserID != "admin-1" {
		t.Fatalf("provision request: %+v", req)
	}
	if req.OperationID != scimOperationID(org, rec.SCIMID, true, 1) {
		t.Fatalf("operation id %q is not derived from (org, scim id, target, revision)", req.OperationID)
	}
	if n := h.outboxCount(t, org, IdentityEventSCIMUserProvisioned); n != 1 {
		t.Fatalf("provisioned audit rows = %d, want 1", n)
	}

	// A second POST for the same userName is a uniqueness conflict, whatever its case.
	if _, err := h.service.CreateSCIMUser(h.ctx, auth, SCIMUserInput{UserName: "ADA@create.example.com"}); !errors.Is(err, ErrSCIMUniqueness) {
		t.Fatalf("duplicate userName = %v, want ErrSCIMUniqueness", err)
	}
}

func TestSCIMCreateInactiveUserDoesNotCallAuthCore(t *testing.T) {
	h := newScimHarness(t)
	org := h.orgWithDomain(t, "scim-inactive", "inactive.example.com")
	auth := h.authFor(t, org)
	off := false
	rec, err := h.service.CreateSCIMUser(h.ctx, auth, SCIMUserInput{UserName: "z@inactive.example.com", Active: &off})
	if err != nil || rec.Active || rec.UserID != "" {
		t.Fatalf("inactive create: %+v, %v", rec, err)
	}
	if len(h.prov.provisioned) != 0 {
		t.Fatalf("an inactive user was provisioned: %+v", h.prov.provisioned)
	}
}

func TestSCIMDeactivateAndReactivateCompareStateFirst(t *testing.T) {
	h := newScimHarness(t)
	org := h.orgWithDomain(t, "scim-noop", "noop.example.com")
	auth := h.authFor(t, org)
	rec, err := h.service.CreateSCIMUser(h.ctx, auth, SCIMUserInput{UserName: "n@noop.example.com"})
	if err != nil {
		t.Fatal(err)
	}

	// active=true on an active user: nothing happens.
	same, err := h.service.SetSCIMUserActive(h.ctx, auth, rec.SCIMID, true)
	if err != nil || same.Revision != rec.Revision || len(h.prov.provisioned) != 1 {
		t.Fatalf("active=true on an active user: %+v, %v, provision calls %d", same, err, len(h.prov.provisioned))
	}

	off, err := h.service.SetSCIMUserActive(h.ctx, auth, rec.SCIMID, false)
	if err != nil || off.Active || off.Revision != rec.Revision+1 || off.DeprovisionedAt == nil {
		t.Fatalf("deactivate: %+v, %v", off, err)
	}
	if len(h.prov.deprovisioned) != 1 {
		t.Fatalf("deprovision calls = %d", len(h.prov.deprovisioned))
	}
	if h.prov.deprovisioned[0].OperationID != scimOperationID(org, rec.SCIMID, false, rec.Revision) {
		t.Fatalf("deprovision operation id %q", h.prov.deprovisioned[0].OperationID)
	}

	// A replayed active=false is a no-op: no second call, no new revision, no second audit row.
	again, err := h.service.SetSCIMUserActive(h.ctx, auth, rec.SCIMID, false)
	if err != nil || again.Revision != off.Revision || len(h.prov.deprovisioned) != 1 {
		t.Fatalf("replayed deactivate: %+v, %v, calls %d", again, err, len(h.prov.deprovisioned))
	}
	if n := h.outboxCount(t, org, IdentityEventSCIMUserDeprovisioned); n != 1 {
		t.Fatalf("deprovisioned audit rows = %d, want 1", n)
	}
}

func TestSCIMReactivationRestoresTheRoleCappedAtAdmin(t *testing.T) {
	cases := []struct {
		deactivatedAs string
		want          string
	}{
		{deactivatedAs: "owner", want: "admin"}, // SCIM never restores an owner
		{deactivatedAs: "admin", want: "admin"},
		{deactivatedAs: "member", want: "member"},
		{deactivatedAs: "viewer", want: "viewer"},
		{deactivatedAs: "superuser", want: "member"}, // unknown roles fall back to member
	}
	for _, tc := range cases {
		t.Run(tc.deactivatedAs, func(t *testing.T) {
			h := newScimHarness(t)
			org := h.orgWithDomain(t, "scim-react-"+tc.deactivatedAs, "react.example.com")
			auth := h.authFor(t, org)
			rec, err := h.service.CreateSCIMUser(h.ctx, auth, SCIMUserInput{UserName: "r@react.example.com"})
			if err != nil {
				t.Fatal(err)
			}
			h.prov.deprovRole = tc.deactivatedAs
			off, err := h.service.SetSCIMUserActive(h.ctx, auth, rec.SCIMID, false)
			if err != nil || off.RoleAtDeactivation != tc.deactivatedAs {
				t.Fatalf("deactivate: %+v, %v", off, err)
			}
			on, err := h.service.SetSCIMUserActive(h.ctx, auth, rec.SCIMID, true)
			if err != nil || !on.Active || on.DeprovisionedAt != nil {
				t.Fatalf("reactivate: %+v, %v", on, err)
			}
			last := h.prov.provisioned[len(h.prov.provisioned)-1]
			if last.Role != tc.want {
				t.Fatalf("reactivation asked for role %q, want %q", last.Role, tc.want)
			}
			if n := h.outboxCount(t, org, IdentityEventSCIMUserReactivated); n != 1 {
				t.Fatalf("reactivated audit rows = %d, want 1", n)
			}
		})
	}
}

func TestSCIMDeactivatingTheLastOwnerIsRefusedAndAudited(t *testing.T) {
	h := newScimHarness(t)
	org := h.orgWithDomain(t, "scim-lastowner", "owner.example.com")
	auth := h.authFor(t, org)
	rec, err := h.service.CreateSCIMUser(h.ctx, auth, SCIMUserInput{UserName: "boss@owner.example.com"})
	if err != nil {
		t.Fatal(err)
	}
	h.prov.deprovErr = ErrSCIMLastOwner
	if _, err := h.service.SetSCIMUserActive(h.ctx, auth, rec.SCIMID, false); !errors.Is(err, ErrSCIMLastOwner) {
		t.Fatalf("deactivating the last owner = %v, want ErrSCIMLastOwner", err)
	}
	if n := h.outboxCount(t, org, IdentityEventSCIMRefusedLastOwner); n != 1 {
		t.Fatalf("refused_last_owner audit rows = %d, want 1", n)
	}
	// The record is untouched: still active, same revision.
	now, err := h.service.GetSCIMUser(h.ctx, auth, rec.SCIMID)
	if err != nil || !now.Active || now.Revision != rec.Revision {
		t.Fatalf("record after a refusal: %+v, %v", now, err)
	}
	// Any other auth-core failure is surfaced and writes nothing.
	h.prov.deprovErr = errors.New("auth-core down")
	if _, err := h.service.SetSCIMUserActive(h.ctx, auth, rec.SCIMID, false); err == nil {
		t.Fatal("an auth-core failure must fail the PATCH so the IdP retries")
	}
	if n := h.outboxCount(t, org, IdentityEventSCIMRefusedLastOwner); n != 1 {
		t.Fatalf("a non-owner failure must not add a refused_last_owner row, have %d", n)
	}
}

func TestSCIMDeleteKeepsATombstoneIsIdempotentAndReviveReusesTheId(t *testing.T) {
	h := newScimHarness(t)
	org := h.orgWithDomain(t, "scim-delete", "delete.example.com")
	auth := h.authFor(t, org)
	rec, err := h.service.CreateSCIMUser(h.ctx, auth, SCIMUserInput{UserName: "d@delete.example.com"})
	if err != nil {
		t.Fatal(err)
	}

	if err := h.service.DeleteSCIMUser(h.ctx, auth, rec.SCIMID); err != nil {
		t.Fatal(err)
	}
	if len(h.prov.deprovisioned) != 1 {
		t.Fatalf("delete of an active user must deprovision once, calls = %d", len(h.prov.deprovisioned))
	}
	// DELETE is a tombstone: the row stays, hidden from reads and lists.
	var deleted bool
	if err := h.db.Pool.QueryRow(h.ctx, `SELECT deleted_at IS NOT NULL AND NOT active FROM organization_scim_users WHERE org_id = $1 AND scim_id = $2::uuid`, org, rec.SCIMID).Scan(&deleted); err != nil || !deleted {
		t.Fatalf("tombstone row: %v, %v", deleted, err)
	}
	if got, err := h.service.GetSCIMUser(h.ctx, auth, rec.SCIMID); err != nil || got.DeletedAt == nil {
		t.Fatalf("the tombstone must read back with DeletedAt set: %+v, %v", got, err)
	}
	if _, err := h.service.SetSCIMUserActive(h.ctx, auth, rec.SCIMID, true); !errors.Is(err, ErrSCIMUserNotFound) {
		t.Fatalf("patching a tombstone = %v, want ErrSCIMUserNotFound", err)
	}
	users, total, err := h.service.ListSCIMUsers(h.ctx, auth, nil, 1, 100)
	if err != nil || total != 0 || len(users) != 0 {
		t.Fatalf("list after delete: %+v total %d, %v", users, total, err)
	}

	// A repeated DELETE and a DELETE of an unknown id both succeed without another auth-core call.
	if err := h.service.DeleteSCIMUser(h.ctx, auth, rec.SCIMID); err != nil {
		t.Fatalf("second delete = %v", err)
	}
	if err := h.service.DeleteSCIMUser(h.ctx, auth, "11111111-2222-3333-4444-555555555555"); err != nil {
		t.Fatalf("delete of an unknown id = %v", err)
	}
	if len(h.prov.deprovisioned) != 1 {
		t.Fatalf("idempotent deletes called auth-core %d times", len(h.prov.deprovisioned))
	}

	// POST with the same userName revives the tombstone under its old id with a higher revision.
	revived, err := h.service.CreateSCIMUser(h.ctx, auth, SCIMUserInput{UserName: "d@delete.example.com"})
	if err != nil {
		t.Fatalf("revive: %v", err)
	}
	if revived.SCIMID != rec.SCIMID || !revived.Active || revived.DeletedAt != nil || revived.Revision <= rec.Revision+1 {
		t.Fatalf("revived record: %+v (original %+v)", revived, rec)
	}
}

func TestSCIMDeleteOfAnInactiveUserKeepsATombstoneWithoutCallingAuthCore(t *testing.T) {
	h := newScimHarness(t)
	org := h.orgWithDomain(t, "scim-delete-off", "deleteoff.example.com")
	auth := h.authFor(t, org)
	off := false
	rec, err := h.service.CreateSCIMUser(h.ctx, auth, SCIMUserInput{UserName: "o@deleteoff.example.com", Active: &off})
	if err != nil {
		t.Fatal(err)
	}
	if err := h.service.DeleteSCIMUser(h.ctx, auth, rec.SCIMID); err != nil {
		t.Fatal(err)
	}
	if len(h.prov.deprovisioned) != 0 {
		t.Fatalf("deleting an inactive user called auth-core %d times", len(h.prov.deprovisioned))
	}
	got, err := h.service.GetSCIMUser(h.ctx, auth, rec.SCIMID)
	if err != nil || got.DeletedAt == nil {
		t.Fatalf("tombstone: %+v, %v", got, err)
	}
}

func TestSCIMListFiltersByUserNameAndExternalIdAndCapsTheCount(t *testing.T) {
	h := newScimHarness(t)
	org := h.orgWithDomain(t, "scim-filter", "filter.example.com")
	auth := h.authFor(t, org)
	for i := 0; i < 3; i++ {
		if _, err := h.service.CreateSCIMUser(h.ctx, auth, SCIMUserInput{
			UserName: fmt.Sprintf("u%d@filter.example.com", i), ExternalID: fmt.Sprintf("ext-%d", i),
		}); err != nil {
			t.Fatal(err)
		}
	}
	byName, total, err := h.service.ListSCIMUsers(h.ctx, auth, &SCIMFilter{Attribute: "userName", Value: " U1@Filter.Example.com "}, 1, 10)
	if err != nil || total != 1 || len(byName) != 1 || byName[0].UserName != "u1@filter.example.com" {
		t.Fatalf("filter userName: %+v total %d, %v", byName, total, err)
	}
	byExt, total, err := h.service.ListSCIMUsers(h.ctx, auth, &SCIMFilter{Attribute: "externalId", Value: "ext-2"}, 1, 10)
	if err != nil || total != 1 || len(byExt) != 1 || byExt[0].ExternalID != "ext-2" {
		t.Fatalf("filter externalId: %+v total %d, %v", byExt, total, err)
	}
	page, total, err := h.service.ListSCIMUsers(h.ctx, auth, nil, 2, 1)
	if err != nil || total != 3 || len(page) != 1 || page[0].UserName != "u1@filter.example.com" {
		t.Fatalf("page 2 of size 1: %+v total %d, %v", page, total, err)
	}
	none, total, err := h.service.ListSCIMUsers(h.ctx, auth, nil, 1, 0)
	if err != nil || total != 3 || len(none) != 0 {
		t.Fatalf("count=0 must return only the total: %+v total %d, %v", none, total, err)
	}
}

func TestSCIMUsersOfOneOrganisationAreNotVisibleToAnother(t *testing.T) {
	h := newScimHarness(t)
	orgA := h.orgWithDomain(t, "scim-iso-a", "iso-a.example.com")
	orgB := h.orgWithDomain(t, "scim-iso-b", "iso-b.example.com")
	authA, authB := h.authFor(t, orgA), h.authFor(t, orgB)
	rec, err := h.service.CreateSCIMUser(h.ctx, authA, SCIMUserInput{UserName: "a@iso-a.example.com"})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := h.service.GetSCIMUser(h.ctx, authB, rec.SCIMID); !errors.Is(err, ErrSCIMUserNotFound) {
		t.Fatalf("cross-org read = %v, want ErrSCIMUserNotFound", err)
	}
	if err := h.service.DeleteSCIMUser(h.ctx, authB, rec.SCIMID); err != nil {
		t.Fatalf("cross-org delete answers as an unknown id: %v", err)
	}
	still, err := h.service.GetSCIMUser(h.ctx, authA, rec.SCIMID)
	if err != nil || !still.Active || still.DeletedAt != nil {
		t.Fatalf("org A's user was changed by org B: %+v, %v", still, err)
	}
}

// --- table-level guarantees ---

func TestSCIMRowLevelSecurityIsolatesBothTables(t *testing.T) {
	h := newScimHarness(t)
	org := h.orgWithDomain(t, "scim-rls", "rls.example.com")
	auth := h.authFor(t, org)
	if _, err := h.service.CreateSCIMUser(h.ctx, auth, SCIMUserInput{UserName: "x@rls.example.com"}); err != nil {
		t.Fatal(err)
	}
	other := "org-scim-rls-other"
	for _, table := range []string{"organization_scim_tokens", "organization_scim_users"} {
		err := h.db.WithOrgScope(h.ctx, other, func(tx pgx.Tx) error {
			var n int
			if err := tx.QueryRow(h.ctx, `SELECT COUNT(*) FROM `+table).Scan(&n); err != nil {
				return err
			}
			if n != 0 {
				t.Fatalf("%s: another tenant's scope sees %d rows", table, n)
			}
			return nil
		})
		if err != nil {
			t.Fatalf("%s cross-tenant read: %v", table, err)
		}
	}
	forgedToken := h.db.WithOrgScope(h.ctx, other, func(tx pgx.Tx) error {
		_, err := tx.Exec(h.ctx, `
INSERT INTO organization_scim_tokens (id, org_id, name, token_prefix, token_hash, created_by, expires_at)
VALUES ('forged-token', $1, 'forged', 'scim_xxxx', repeat('a', 64), 'attacker', NOW() + INTERVAL '1 day')`, org)
		return err
	})
	if forgedToken == nil {
		t.Fatal("a scoped transaction wrote a token into another organisation")
	}
	forgedUser := h.db.WithOrgScope(h.ctx, other, func(tx pgx.Tx) error {
		_, err := tx.Exec(h.ctx, `
INSERT INTO organization_scim_users (org_id, scim_id, user_name, active) VALUES ($1, '99999999-9999-4999-8999-999999999999', 'forged@rls.example.com', true)`, org)
		return err
	})
	if forgedUser == nil {
		t.Fatal("a scoped transaction wrote a SCIM user into another organisation")
	}
	// The app role cannot hard delete either table.
	for _, table := range []string{"organization_scim_tokens", "organization_scim_users"} {
		del := h.db.WithOrgScope(h.ctx, org, func(tx pgx.Tx) error {
			_, err := tx.Exec(h.ctx, `DELETE FROM `+table+` WHERE org_id = $1`, org)
			return err
		})
		if del == nil {
			t.Fatalf("the app role must not be able to DELETE from %s", table)
		}
	}
}

func TestSCIMUserNameIsUniquePerOrganisationAtTheTable(t *testing.T) {
	h := newScimHarness(t)
	orgA := h.orgWithDomain(t, "scim-uniq-a", "uniq-a.example.com")
	orgB := h.orgWithDomain(t, "scim-uniq-b", "uniq-b.example.com")
	insert := func(org, id string) error {
		return h.db.WithOrgScope(h.ctx, org, func(tx pgx.Tx) error {
			_, err := tx.Exec(h.ctx, `INSERT INTO organization_scim_users (org_id, scim_id, user_name, active) VALUES ($1, $2::uuid, 'same@example.com', true)`, org, id)
			return err
		})
	}
	if err := insert(orgA, "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"); err != nil {
		t.Fatal(err)
	}
	if err := insert(orgA, "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"); err == nil {
		t.Fatal("(org_id, user_name) must be unique")
	}
	if err := insert(orgB, "cccccccc-cccc-4ccc-8ccc-cccccccccccc"); err != nil {
		t.Fatalf("the same userName in another organisation is allowed: %v", err)
	}
}

func TestSCIMUpsertRevisionCASConflictAndAuditInTheSameTransaction(t *testing.T) {
	h := newScimHarness(t)
	org := h.orgWithDomain(t, "scim-cas", "cas.example.com")
	auth := h.authFor(t, org)
	rec, err := h.service.CreateSCIMUser(h.ctx, auth, SCIMUserInput{UserName: "c@cas.example.com"})
	if err != nil {
		t.Fatal(err)
	}
	// A writer holding a stale revision (it expects rec.Revision-1 to be the row's) conflicts.
	_, err = h.repo.upsertSCIMUser(h.ctx, auth, scimUserWrite{
		SCIMID: rec.SCIMID, UserName: rec.UserName, UserID: rec.UserID, Active: false,
		Revision: rec.Revision, Update: true, RoleAtDeactivation: "member",
		Event: IdentityEventSCIMUserDeprovisioned, EventOn: true,
	})
	if !errors.Is(err, ErrSCIMMembershipConflict) {
		t.Fatalf("stale revision = %v, want ErrSCIMMembershipConflict", err)
	}
	if n := h.outboxCount(t, org, IdentityEventSCIMUserDeprovisioned); n != 0 {
		t.Fatalf("a conflicting write left %d audit rows (the audit insert must share the transaction)", n)
	}
	// The correct revision succeeds and writes exactly one audit row.
	ok, err := h.repo.upsertSCIMUser(h.ctx, auth, scimUserWrite{
		SCIMID: rec.SCIMID, UserName: rec.UserName, UserID: rec.UserID, Active: false,
		Revision: rec.Revision + 1, Update: true, RoleAtDeactivation: "member",
		Event: IdentityEventSCIMUserDeprovisioned, EventOn: true,
	})
	if err != nil || ok.Revision != rec.Revision+1 {
		t.Fatalf("matching revision: %+v, %v", ok, err)
	}
	if n := h.outboxCount(t, org, IdentityEventSCIMUserDeprovisioned); n != 1 {
		t.Fatalf("audit rows = %d, want 1", n)
	}
}

func TestMigration029AppliesIdempotentlyAndRollsBack(t *testing.T) {
	h := newScimHarness(t)
	up, err := os.ReadFile("../../migrations/029_org_scim.up.sql")
	if err != nil {
		t.Fatal(err)
	}
	down, err := os.ReadFile("../../migrations/029_org_scim.down.sql")
	if err != nil {
		t.Fatal(err)
	}
	exists := func(table string) bool {
		var ok bool
		if err := h.db.Pool.QueryRow(h.ctx, `SELECT to_regclass($1) IS NOT NULL`, "public."+table).Scan(&ok); err != nil {
			t.Fatal(err)
		}
		return ok
	}
	// Additive and idempotent: applying it again over live tables must succeed.
	if _, err := h.db.Pool.Exec(h.ctx, string(up)); err != nil {
		t.Fatalf("re-apply 029: %v", err)
	}
	for _, table := range []string{"organization_scim_tokens", "organization_scim_users"} {
		var rls bool
		if err := h.db.Pool.QueryRow(h.ctx, `SELECT relrowsecurity FROM pg_class WHERE oid = to_regclass($1)`, "public."+table).Scan(&rls); err != nil || !rls {
			t.Fatalf("%s: row level security enabled = %v, %v", table, rls, err)
		}
	}
	if _, err := h.db.Pool.Exec(h.ctx, string(down)); err != nil {
		t.Fatalf("apply 029 down: %v", err)
	}
	if exists("organization_scim_tokens") || exists("organization_scim_users") {
		t.Fatal("the down migration must drop both tables")
	}
	if _, err := h.db.Pool.Exec(h.ctx, string(up)); err != nil {
		t.Fatalf("apply 029 up after down: %v", err)
	}
	if !exists("organization_scim_tokens") || !exists("organization_scim_users") {
		t.Fatal("the up migration must recreate both tables")
	}
}
