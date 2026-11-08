package org

import (
	"errors"
	"strings"
	"testing"
)

// WS05-S7: the SSO policy repository and the enforce flow against a real Postgres. Gated like
// domain_postgres_test.go (CONTROL_LIFECYCLE_TEST_DATABASE_URL); deferred to the DB-harness tier.

func (h *identityHarness) withAdmin(admin *fakeIdentityAdmin) *fakeIdentityAdmin {
	h.service.SetIdentityProviderAdmin(admin)
	return admin
}

func ssoEvents(t *testing.T, h *identityHarness, orgID, event string) []string {
	t.Helper()
	rows, err := h.db.Pool.Query(h.ctx, `
SELECT payload::text FROM organization_identity_audit_outbox
WHERE org_id = $1 AND subject = $2 ORDER BY created_at`, orgID, identityAuditSubjectPrefix+event)
	if err != nil {
		t.Fatal(err)
	}
	defer rows.Close()
	var out []string
	for rows.Next() {
		var payload string
		if err := rows.Scan(&payload); err != nil {
			t.Fatal(err)
		}
		out = append(out, payload)
	}
	return out
}

func TestSSOPolicyCompareAndSetWithOutboxInTheSameTransaction(t *testing.T) {
	h := newIdentityHarness(t)
	h.withAdmin(&fakeIdentityAdmin{facts: []SSOMemberFact{ownerWithTwoFactor("owner-bg")}})
	org := h.newOrg(t, "sso-cas")

	fresh, err := h.service.GetSSOPolicy(h.ctx, org, "admin-1")
	if err != nil || fresh.Mode != SSOModeOff || fresh.Revision != 0 || len(fresh.Domains) != 0 {
		t.Fatalf("no row means off at revision 0: %+v, %v", fresh, err)
	}
	if fresh.CanEnforce.OK || len(fresh.CanEnforce.Reasons) == 0 || fresh.CanEnforce.Reasons[0] != SSOReasonNoVerifiedDomain {
		t.Fatalf("can_enforce on a fresh org: %+v", fresh.CanEnforce)
	}

	// Insert path: expected revision 0 creates revision 1.
	first, err := h.service.PutSSOPolicy(h.ctx, org, "admin-1", "admin", SSOPolicyInput{Mode: SSOModeReport, ExpectedRevision: 0, BreakGlassUserIDs: []string{"owner-bg"}})
	if err != nil || first.Mode != SSOModeReport || first.Revision != 1 || len(first.BreakGlassUserIDs) != 1 {
		t.Fatalf("insert: %+v, %v", first, err)
	}
	// A stale writer loses.
	if _, err := h.service.PutSSOPolicy(h.ctx, org, "admin-2", "admin", SSOPolicyInput{Mode: SSOModeOff, ExpectedRevision: 0}); !errors.Is(err, ErrSSOPolicyRevisionConflict) {
		t.Fatalf("stale write: %v, want ErrSSOPolicyRevisionConflict", err)
	}
	// Update path.
	second, err := h.service.PutSSOPolicy(h.ctx, org, "admin-1", "admin", SSOPolicyInput{Mode: SSOModeOff, ExpectedRevision: 1})
	if err != nil || second.Revision != 2 || second.Mode != SSOModeOff || len(second.BreakGlassUserIDs) != 0 {
		t.Fatalf("update: %+v, %v", second, err)
	}

	var mode, updatedBy string
	var revision int64
	if err := h.db.Pool.QueryRow(h.ctx, `SELECT mode, revision, updated_by FROM organization_sso_policy WHERE org_id = $1`, org).Scan(&mode, &revision, &updatedBy); err != nil {
		t.Fatal(err)
	}
	if mode != SSOModeOff || revision != 2 || updatedBy != "admin-1" {
		t.Fatalf("stored policy: %s r%d by %s", mode, revision, updatedBy)
	}

	// Exactly one outbox row per accepted change, none for the refused write.
	events := ssoEvents(t, h, org, IdentityEventSSOPolicyUpdated)
	if len(events) != 2 {
		t.Fatalf("policy audit rows = %d, want 2", len(events))
	}
	for _, payload := range events {
		for _, want := range []string{`"org_sso_policy_updated"`, `"sso_policy:sso_policy"`, `"mode"`} {
			if !strings.Contains(payload, want) {
				t.Fatalf("payload lacks %s: %s", want, payload)
			}
		}
	}

	// More than two break-glass ids is refused by the table CHECK as well as the service.
	if _, err := h.db.Pool.Exec(h.ctx, `UPDATE organization_sso_policy SET break_glass_user_ids = ARRAY['a','b','c'] WHERE org_id = $1`, org); err == nil {
		t.Fatal("the table must refuse three break-glass users")
	}
}

func TestSSOEnforceChecksTheMissingPreconditionFirstInOrder(t *testing.T) {
	h := newIdentityHarness(t)
	admin := h.withAdmin(&fakeIdentityAdmin{})
	org := h.newOrg(t, "sso-enforce")
	put := func(revision int64, breakGlass ...string) (*SSOPolicyView, error) {
		return h.service.PutSSOPolicy(h.ctx, org, "admin-1", "admin", SSOPolicyInput{Mode: SSOModeEnforce, ExpectedRevision: revision, BreakGlassUserIDs: breakGlass})
	}
	wantCode := func(label, code string, err error) {
		t.Helper()
		var enforce *SSOEnforceError
		if !errors.As(err, &enforce) || enforce.Code != code {
			t.Fatalf("%s: err = %v, want SSOEnforceError %s", label, err, code)
		}
	}

	_, err := put(0)
	wantCode("no domain", SSOReasonNoVerifiedDomain, err)

	h.addAndVerify(t, org, "sso-enforce.example.com")
	_, err = put(0)
	wantCode("no provider", SSOReasonNoProvider, err)

	admin.providers = []SSOProviderRecord{{ProviderID: "acme-oidc", Domain: "sso-enforce.example.com", Issuer: "https://idp.example.com", Type: "oidc"}}
	_, err = put(0)
	wantCode("own sign-in not proven", SSOReasonNotProven, err)

	admin.signedIn = true
	_, err = put(0)
	wantCode("no break-glass user", SSOReasonBreakGlass, err)

	admin.facts = []SSOMemberFact{{UserID: "owner-bg", Role: "owner"}}
	_, err = put(0, "owner-bg")
	wantCode("break-glass without 2fa", SSOReasonBreakGlass2FA, err)
	if n := len(ssoEvents(t, h, org, IdentityEventSSOPolicyUpdated)); n != 0 {
		t.Fatalf("a refused enforce must leave no audit row, have %d", n)
	}

	admin.facts = []SSOMemberFact{ownerWithTwoFactor("owner-bg")}
	view, err := put(0, "owner-bg")
	if err != nil || view.Mode != SSOModeEnforce || view.Revision != 1 {
		t.Fatalf("enforce: %+v, %v", view, err)
	}
	// The sweep that ends password sessions ran once, for the verified domains, sparing the
	// break-glass owner and the acting admin.
	if admin.revokeCalls != 1 || len(admin.revokedDomains) != 1 || admin.revokedDomains[0] != "sso-enforce.example.com" {
		t.Fatalf("revoke sweep: calls=%d domains=%v", admin.revokeCalls, admin.revokedDomains)
	}
	exempt := strings.Join(admin.revokeExempt, ",")
	if !strings.Contains(exempt, "owner-bg") || !strings.Contains(exempt, "admin-1") {
		t.Fatalf("exempt users = %v", admin.revokeExempt)
	}
	// Saving enforce again does not sweep a second time.
	if _, err := put(1, "owner-bg"); err != nil {
		t.Fatalf("re-save enforce: %v", err)
	}
	if admin.revokeCalls != 1 {
		t.Fatalf("an enforce -> enforce save swept again: %d", admin.revokeCalls)
	}

	// A non-owner break-glass user is refused in report mode too.
	other := h.newOrg(t, "sso-bg-owner")
	h.withAdmin(&fakeIdentityAdmin{facts: []SSOMemberFact{{UserID: "plain-admin", Role: "admin", TwoFactorEnabled: true}}})
	_, err = h.service.PutSSOPolicy(h.ctx, other, "admin-1", "admin", SSOPolicyInput{Mode: SSOModeReport, ExpectedRevision: 0, BreakGlassUserIDs: []string{"plain-admin"}})
	wantCode("report with a non-owner break-glass user", SSOReasonBreakGlass, err)
}

func TestSSOWritePolicyRefusesEnforceWhenTheLastDomainWasRevokedInTheMeantime(t *testing.T) {
	h := newIdentityHarness(t)
	org := h.newOrg(t, "sso-race")
	// The repository re-checks inside its transaction; call it as the service would after a stale precondition read.
	err := h.repo.writeSSOPolicy(h.ctx, org, "admin-1", "admin", SSOModeEnforce, nil, 0)
	var enforce *SSOEnforceError
	if !errors.As(err, &enforce) || enforce.Code != SSOReasonNoVerifiedDomain {
		t.Fatalf("enforce with no verified domain: %v", err)
	}
	var rows int
	if err := h.db.Pool.QueryRow(h.ctx, `SELECT COUNT(*) FROM organization_sso_policy WHERE org_id = $1`, org).Scan(&rows); err != nil || rows != 0 {
		t.Fatalf("a refused write left %d policy rows, err %v", rows, err)
	}
}

func TestSSOPolicyByDomainLookup(t *testing.T) {
	h := newIdentityHarness(t)
	h.withAdmin(&fakeIdentityAdmin{facts: []SSOMemberFact{ownerWithTwoFactor("owner-bg")}})
	org := h.newOrg(t, "sso-lookup")
	domain := "sso-lookup.example.com"

	// Unverified (pending) domains have no covering policy.
	if _, err := h.service.AddIdentityDomain(h.ctx, org, "admin-1", "admin", domain); err != nil {
		t.Fatal(err)
	}
	if _, err := h.service.LookupSSOPolicyByDomain(h.ctx, domain); !errors.Is(err, ErrDomainNotFound) {
		t.Fatalf("pending domain: %v", err)
	}
	if _, err := h.service.LookupSSOPolicyByDomain(h.ctx, "unknown-domain.example.com"); !errors.Is(err, ErrDomainNotFound) {
		t.Fatalf("unknown domain: %v", err)
	}

	// Verified, no policy row: mode off, revision 0, no break-glass users.
	h.publish(t, org, domain)
	if res, err := h.service.VerifyIdentityDomain(h.ctx, org, "admin-1", "admin", domain); err != nil || res.Status != "verified" {
		t.Fatalf("verify: %+v, %v", res, err)
	}
	none, err := h.service.LookupSSOPolicyByDomain(h.ctx, "SSO-Lookup.Example.com.")
	if err != nil || none.OrgID != org || none.Mode != SSOModeOff || none.Revision != 0 || none.BreakGlassUserIDs == nil || len(none.BreakGlassUserIDs) != 0 {
		t.Fatalf("verified, no policy: %+v, %v", none, err)
	}

	// With a policy row.
	if _, err := h.service.PutSSOPolicy(h.ctx, org, "admin-1", "admin", SSOPolicyInput{Mode: SSOModeReport, ExpectedRevision: 0, BreakGlassUserIDs: []string{"owner-bg"}}); err != nil {
		t.Fatal(err)
	}
	with, err := h.service.LookupSSOPolicyByDomain(h.ctx, domain)
	if err != nil || with.Mode != SSOModeReport || with.Revision != 1 || len(with.BreakGlassUserIDs) != 1 || with.BreakGlassUserIDs[0] != "owner-bg" {
		t.Fatalf("with policy: %+v, %v", with, err)
	}

	// A soft-deleted organisation stops covering its domain.
	if _, err := h.db.Pool.Exec(h.ctx, `UPDATE organizations SET deleted_at = NOW() WHERE id = $1`, org); err != nil {
		t.Fatal(err)
	}
	if _, err := h.service.LookupSSOPolicyByDomain(h.ctx, domain); !errors.Is(err, ErrDomainNotFound) {
		t.Fatalf("soft-deleted org: %v", err)
	}
}

func TestSSOProviderLifecycleAuditsWithoutTheSecret(t *testing.T) {
	h := newIdentityHarness(t)
	admin := h.withAdmin(&fakeIdentityAdmin{signedIn: true, facts: []SSOMemberFact{ownerWithTwoFactor("owner-bg")}})
	org := h.newOrg(t, "sso-provider")
	domain := "sso-provider.example.com"
	secret := "client-secret-ssssssssssssssssssss"
	in := SSOProviderInput{ProviderID: "Acme-OIDC", Domain: domain, Type: "oidc", Issuer: "https://idp.example.com", ClientID: "client-1", ClientSecret: secret}

	// A domain the org has not proven is refused before auth-core is called.
	if _, err := h.service.AddSSOProvider(h.ctx, org, "admin-1", "admin", in); !errors.Is(err, ErrSSODomainNotVerified) {
		t.Fatalf("unverified domain: %v", err)
	}
	if len(admin.registered) != 0 {
		t.Fatal("an unverified domain reached auth-core")
	}

	h.addAndVerify(t, org, domain)
	provider, err := h.service.AddSSOProvider(h.ctx, org, "admin-1", "admin", in)
	if err != nil || provider.ProviderID != "acme-oidc" || provider.Domain != domain || !provider.DomainVerified {
		t.Fatalf("add: %+v, %v", provider, err)
	}
	if len(admin.registered) != 1 || admin.registered[0].ClientSecret != secret || admin.registered[0].OrganizationID != org {
		t.Fatalf("auth-core registration: %+v", admin.registered)
	}
	added := ssoEvents(t, h, org, IdentityEventSSOProviderAdded)
	if len(added) != 1 || strings.Contains(added[0], secret) || strings.Contains(added[0], "client-1") {
		t.Fatalf("provider-added audit must exist and carry no credential: %v", added)
	}

	// auth-core says the id or domain is taken: no audit row.
	admin.registerErr = ErrSSOProviderExists
	if _, err := h.service.AddSSOProvider(h.ctx, org, "admin-1", "admin", in); !errors.Is(err, ErrSSOProviderExists) {
		t.Fatalf("duplicate: %v", err)
	}
	if n := len(ssoEvents(t, h, org, IdentityEventSSOProviderAdded)); n != 1 {
		t.Fatalf("a refused add wrote an audit row: %d", n)
	}
	admin.registerErr = nil

	// Removal: under enforce the last provider on a verified domain cannot go.
	admin.providers = []SSOProviderRecord{{ProviderID: "acme-oidc", Domain: domain, Issuer: "https://idp.example.com", Type: "oidc"}}
	if _, err := h.service.PutSSOPolicy(h.ctx, org, "admin-1", "admin", SSOPolicyInput{Mode: SSOModeEnforce, ExpectedRevision: 0, BreakGlassUserIDs: []string{"owner-bg"}}); err != nil {
		t.Fatalf("enforce: %v", err)
	}
	if err := h.service.RemoveSSOProvider(h.ctx, org, "admin-1", "admin", "acme-oidc"); !errors.Is(err, ErrSSOLastProviderEnforced) {
		t.Fatalf("last provider under enforce: %v", err)
	}
	if len(admin.deleted) != 0 {
		t.Fatal("the refused removal reached auth-core")
	}
	// With a second provider on a verified domain it may go.
	admin.providers = append(admin.providers, SSOProviderRecord{ProviderID: "second-idp", Domain: domain, Issuer: "https://idp2.example.com", Type: "oidc"})
	if err := h.service.RemoveSSOProvider(h.ctx, org, "admin-1", "admin", "acme-oidc"); err != nil {
		t.Fatalf("remove with another provider: %v", err)
	}
	if len(admin.deleted) != 1 || admin.deleted[0] != "acme-oidc" {
		t.Fatalf("deleted: %v", admin.deleted)
	}
	if n := len(ssoEvents(t, h, org, IdentityEventSSOProviderGone)); n != 1 {
		t.Fatalf("provider-removed audit rows = %d", n)
	}
}

func TestSSORevokingTheLastVerifiedDomainIsBlockedUnderEnforce(t *testing.T) {
	h := newIdentityHarness(t)
	h.withAdmin(&fakeIdentityAdmin{
		signedIn:  true,
		providers: []SSOProviderRecord{{ProviderID: "acme-oidc", Domain: "sso-revoke.example.com", Type: "oidc"}},
		facts:     []SSOMemberFact{ownerWithTwoFactor("owner-bg")},
	})
	org := h.newOrg(t, "sso-revoke")
	h.addAndVerify(t, org, "sso-revoke.example.com")
	if _, err := h.service.PutSSOPolicy(h.ctx, org, "admin-1", "admin", SSOPolicyInput{Mode: SSOModeEnforce, ExpectedRevision: 0, BreakGlassUserIDs: []string{"owner-bg"}}); err != nil {
		t.Fatalf("enforce: %v", err)
	}
	if err := h.service.RevokeIdentityDomain(h.ctx, org, "admin-1", "admin", "sso-revoke.example.com"); !errors.Is(err, ErrDomainInUse) {
		t.Fatalf("revoke under enforce: %v, want ErrDomainInUse", err)
	}
	// Lowering the policy releases the domain.
	if _, err := h.service.PutSSOPolicy(h.ctx, org, "admin-1", "admin", SSOPolicyInput{Mode: SSOModeReport, ExpectedRevision: 1}); err != nil {
		t.Fatal(err)
	}
	if err := h.service.RevokeIdentityDomain(h.ctx, org, "admin-1", "admin", "sso-revoke.example.com"); err != nil {
		t.Fatalf("revoke after lowering: %v", err)
	}
}
