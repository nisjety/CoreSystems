package org

import (
	"context"
	"errors"
	"fmt"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/testfixture"
)

// WS05-S6 (and WS05-S7): the domain claim and SSO policy repositories against a real Postgres.
// Gated like the other lifecycle tests: they skip unless CONTROL_LIFECYCLE_TEST_DATABASE_URL names a
// disposable, marked database. Deferred to the DB-harness tier.

type identityHarness struct {
	ctx      context.Context
	db       *database.DB
	repo     *Repository
	service  *Service
	resolver *fakeTXTResolver
}

func newIdentityHarness(t *testing.T) *identityHarness {
	t.Helper()
	dsn := os.Getenv("CONTROL_LIFECYCLE_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("CONTROL_LIFECYCLE_TEST_DATABASE_URL is not set")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 90*time.Second)
	t.Cleanup(cancel)
	db, err := database.Connect(ctx, dsn)
	if err != nil {
		t.Fatalf("connect to disposable Postgres: %v", err)
	}
	t.Cleanup(db.Close)
	if err := testfixture.VerifyLifecycleMarker(ctx, db.Pool, dsn, "org_lifecycle", os.Getenv("CONTROL_LIFECYCLE_FIXTURE_ID")); err != nil {
		t.Fatalf("refusing unsafe lifecycle database: %v", err)
	}
	if err := database.RunMigrations(ctx, db, "../../migrations"); err != nil {
		t.Fatalf("apply org-core migrations: %v", err)
	}
	repo := NewRepository(db)
	service := NewService(repo, nil)
	resolver := &fakeTXTResolver{records: map[string][]string{}}
	service.EnableDomainVerification(resolver)
	return &identityHarness{ctx: ctx, db: db, repo: repo, service: service, resolver: resolver}
}

var identityOrgSeq int

func (h *identityHarness) newOrg(t *testing.T, label string) string {
	t.Helper()
	identityOrgSeq++
	id := fmt.Sprintf("org-idn-%s-%d-%d", label, time.Now().UnixNano(), identityOrgSeq)
	if err := h.repo.ProvisionOrganizationWithOwner(h.ctx, Organization{
		ID: id, Name: "Identity " + label, Slug: id, Plan: "enterprise", Status: "active",
	}, "owner-"+id); err != nil {
		t.Fatalf("seed organization: %v", err)
	}
	return id
}

func (h *identityHarness) publish(t *testing.T, orgID, domain string) {
	t.Helper()
	domains, err := h.service.ListIdentityDomains(h.ctx, orgID)
	if err != nil {
		t.Fatal(err)
	}
	for _, d := range domains {
		if d.Domain == domain && d.Challenge != nil {
			name := d.Challenge.Name
			h.resolver.records[name] = append(h.resolver.records[name], d.Challenge.Value)
			return
		}
	}
	t.Fatalf("no pending challenge for %s in %s", domain, orgID)
}

func (h *identityHarness) addAndVerify(t *testing.T, orgID, domain string) {
	t.Helper()
	if _, err := h.service.AddIdentityDomain(h.ctx, orgID, "admin-1", "admin", domain); err != nil {
		t.Fatalf("add %s: %v", domain, err)
	}
	h.publish(t, orgID, domain)
	res, err := h.service.VerifyIdentityDomain(h.ctx, orgID, "admin-1", "admin", domain)
	if err != nil || res.Status != "verified" {
		t.Fatalf("verify %s: %+v, %v", domain, res, err)
	}
}

func (h *identityHarness) outboxCount(t *testing.T, orgID, event string) int {
	t.Helper()
	var n int
	if err := h.db.Pool.QueryRow(h.ctx, `
SELECT COUNT(*) FROM organization_identity_audit_outbox
WHERE org_id = $1 AND subject = $2`, orgID, identityAuditSubjectPrefix+event).Scan(&n); err != nil {
		t.Fatalf("count outbox: %v", err)
	}
	return n
}

func TestIdentityDomainAddIsIdempotentAndRefreshesAnExpiredChallenge(t *testing.T) {
	h := newIdentityHarness(t)
	org := h.newOrg(t, "add")

	first, err := h.service.AddIdentityDomain(h.ctx, org, "admin-1", "admin", "Idempotent.Example.com")
	if err != nil || first.Status != "pending" || first.Challenge == nil {
		t.Fatalf("first add: %+v, %v", first, err)
	}
	if first.Domain != "idempotent.example.com" {
		t.Fatalf("domain must be stored normalized, got %q", first.Domain)
	}
	if first.Challenge.Name != "_verevon-challenge.idempotent.example.com" || !strings.HasPrefix(first.Challenge.Value, DomainChallengeValuePrefix) {
		t.Fatalf("challenge: %+v", first.Challenge)
	}

	again, err := h.service.AddIdentityDomain(h.ctx, org, "admin-1", "admin", "idempotent.example.com")
	if err != nil || again.Challenge == nil || again.Challenge.Value != first.Challenge.Value {
		t.Fatalf("a repeat add must return the same live challenge: %+v, %v", again, err)
	}
	if n := h.outboxCount(t, org, IdentityEventDomainAdded); n != 1 {
		t.Fatalf("a no-op add must not enqueue another audit row, have %d", n)
	}

	if _, err := h.db.Pool.Exec(h.ctx, `
UPDATE organization_domains SET verification_expires_at = NOW() - INTERVAL '1 hour'
WHERE org_id = $1 AND normalized_domain = 'idempotent.example.com'`, org); err != nil {
		t.Fatal(err)
	}
	refreshed, err := h.service.AddIdentityDomain(h.ctx, org, "admin-1", "admin", "idempotent.example.com")
	if err != nil || refreshed.Challenge == nil {
		t.Fatalf("refresh: %+v, %v", refreshed, err)
	}
	if refreshed.Challenge.Value == first.Challenge.Value {
		t.Fatal("an expired challenge must be replaced by a new token")
	}
	if !refreshed.Challenge.ExpiresAt.After(time.Now()) {
		t.Fatalf("the new challenge must be live: %+v", refreshed.Challenge)
	}
	if n := h.outboxCount(t, org, IdentityEventDomainAdded); n != 2 {
		t.Fatalf("a refresh is a recorded change, have %d audit rows", n)
	}
}

func TestIdentityDomainCapOfTenCountsPendingAndVerifiedNotRevoked(t *testing.T) {
	h := newIdentityHarness(t)
	org := h.newOrg(t, "cap")
	for i := 1; i <= MaxIdentityDomainsPerOrg; i++ {
		if _, err := h.service.AddIdentityDomain(h.ctx, org, "admin-1", "admin", fmt.Sprintf("cap%d.example.com", i)); err != nil {
			t.Fatalf("add %d: %v", i, err)
		}
	}
	if _, err := h.service.AddIdentityDomain(h.ctx, org, "admin-1", "admin", "cap11.example.com"); !errors.Is(err, ErrDomainLimitReached) {
		t.Fatalf("11th domain: %v, want ErrDomainLimitReached", err)
	}
	// Re-adding one it already holds is not a new claim.
	if _, err := h.service.AddIdentityDomain(h.ctx, org, "admin-1", "admin", "cap3.example.com"); err != nil {
		t.Fatalf("re-adding a held domain at the cap: %v", err)
	}
	if err := h.service.RevokeIdentityDomain(h.ctx, org, "admin-1", "admin", "cap1.example.com"); err != nil {
		t.Fatal(err)
	}
	if _, err := h.service.AddIdentityDomain(h.ctx, org, "admin-1", "admin", "cap11.example.com"); err != nil {
		t.Fatalf("a revoked claim frees a slot: %v", err)
	}
}

func TestIdentityDomainSecondOrgGetsVerifiedElsewhereOnAddAndOnVerify(t *testing.T) {
	h := newIdentityHarness(t)
	orgA := h.newOrg(t, "owner-a")
	orgB := h.newOrg(t, "owner-b")
	orgC := h.newOrg(t, "owner-c")
	domain := fmt.Sprintf("contested-%d.example.com", time.Now().UnixNano())

	// B and C claim first and both publish the record; A then wins the race.
	for _, org := range []string{orgB, orgC} {
		if _, err := h.service.AddIdentityDomain(h.ctx, org, "admin-1", "admin", domain); err != nil {
			t.Fatal(err)
		}
		h.publish(t, org, domain)
	}
	h.addAndVerify(t, orgA, domain)

	// Add by a late org is refused outright.
	late := h.newOrg(t, "late")
	if _, err := h.service.AddIdentityDomain(h.ctx, late, "admin-1", "admin", domain); !errors.Is(err, ErrDomainVerifiedElsewhere) {
		t.Fatalf("add after verified elsewhere: %v", err)
	}
	// A pending claim that proves the record is still refused by the unique index.
	if _, err := h.service.VerifyIdentityDomain(h.ctx, orgB, "admin-1", "admin", domain); !errors.Is(err, ErrDomainVerifiedElsewhere) {
		t.Fatalf("verify of a contested domain: %v, want ErrDomainVerifiedElsewhere", err)
	}
	if n := h.outboxCount(t, orgB, IdentityEventDomainVerified); n != 0 {
		t.Fatalf("a refused verification must leave no audit row, have %d", n)
	}
	if n := h.outboxCount(t, orgC, IdentityEventDomainVerified); n != 0 {
		t.Fatalf("org C audit rows: %d", n)
	}
	owner, err := h.service.LookupVerifiedDomainOwner(h.ctx, domain)
	if err != nil || owner.OrgID != orgA || owner.Status != "verified" {
		t.Fatalf("owner lookup: %+v, %v", owner, err)
	}
}

func TestIdentityDomainVerifyWritesTheOutboxRowInTheSameTransaction(t *testing.T) {
	h := newIdentityHarness(t)
	org := h.newOrg(t, "verify")
	domain := "verify-audit.example.com"
	if _, err := h.service.AddIdentityDomain(h.ctx, org, "admin-1", "admin", domain); err != nil {
		t.Fatal(err)
	}

	// No record published: the attempt is recorded, nothing is verified, no verified audit row.
	res, err := h.service.VerifyIdentityDomain(h.ctx, org, "admin-1", "admin", domain)
	if err != nil || res.Status != "pending" || res.Reason != DomainReasonTXTNotFound {
		t.Fatalf("no record: %+v, %v", res, err)
	}
	domains, _ := h.service.ListIdentityDomains(h.ctx, org)
	if len(domains) == 0 || domains[0].LastCheckReason != DomainReasonTXTNotFound || domains[0].LastCheckAt == nil {
		t.Fatalf("the failed check must be stored: %+v", domains)
	}
	if n := h.outboxCount(t, org, IdentityEventDomainVerified); n != 0 {
		t.Fatalf("verified audit rows before proof: %d", n)
	}

	h.publish(t, org, domain)
	res, err = h.service.VerifyIdentityDomain(h.ctx, org, "admin-1", "admin", domain)
	if err != nil || res.Status != "verified" || res.Reason != "" {
		t.Fatalf("verify: %+v, %v", res, err)
	}
	if n := h.outboxCount(t, org, IdentityEventDomainVerified); n != 1 {
		t.Fatalf("verified audit rows: %d, want 1", n)
	}
	var payload string
	var attempts int
	if err := h.db.Pool.QueryRow(h.ctx, `
SELECT payload::text, attempts FROM organization_identity_audit_outbox
WHERE org_id = $1 AND subject = $2`, org, identityAuditSubjectPrefix+IdentityEventDomainVerified).Scan(&payload, &attempts); err != nil {
		t.Fatal(err)
	}
	for _, want := range []string{`"org_domain_verified"`, `"admin-1"`, `"admin"`, domain, `"control"`, `"org-core"`} {
		if !strings.Contains(payload, want) {
			t.Fatalf("payload lacks %s: %s", want, payload)
		}
	}
	if strings.Contains(payload, DomainChallengeValuePrefix) {
		t.Fatalf("the audit payload must never carry the challenge: %s", payload)
	}
	if attempts != 0 {
		t.Fatalf("a fresh outbox row has attempts=0, got %d", attempts)
	}

	// Verifying again is a no-op that reports verified without a second audit row.
	res, err = h.service.VerifyIdentityDomain(h.ctx, org, "admin-1", "admin", domain)
	if err != nil || res.Status != "verified" {
		t.Fatalf("repeat verify: %+v, %v", res, err)
	}
	if n := h.outboxCount(t, org, IdentityEventDomainVerified); n != 1 {
		t.Fatalf("repeat verify wrote another audit row: %d", n)
	}
}

func TestIdentityDomainVerifyHandlesWrongExpiredAndUnknownClaims(t *testing.T) {
	h := newIdentityHarness(t)
	org := h.newOrg(t, "verify-edge")
	domain := "verify-edge.example.com"
	if _, err := h.service.AddIdentityDomain(h.ctx, org, "admin-1", "admin", domain); err != nil {
		t.Fatal(err)
	}
	h.resolver.records[DomainChallengeName(domain)] = []string{DomainChallengeValue("not-the-token")}
	res, err := h.service.VerifyIdentityDomain(h.ctx, org, "admin-1", "admin", domain)
	if err != nil || res.Status != "pending" || res.Reason != DomainReasonTXTMismatch {
		t.Fatalf("mismatch: %+v, %v", res, err)
	}

	if _, err := h.db.Pool.Exec(h.ctx, `
UPDATE organization_domains SET verification_expires_at = NOW() - INTERVAL '1 minute'
WHERE org_id = $1 AND normalized_domain = $2`, org, domain); err != nil {
		t.Fatal(err)
	}
	res, err = h.service.VerifyIdentityDomain(h.ctx, org, "admin-1", "admin", domain)
	if err != nil || res.Reason != DomainReasonChallengeExpire {
		t.Fatalf("expired: %+v, %v", res, err)
	}

	if _, err := h.service.VerifyIdentityDomain(h.ctx, org, "admin-1", "admin", "never-added.example.com"); !errors.Is(err, ErrDomainNotFound) {
		t.Fatalf("unknown domain verify: %v", err)
	}
	// Another org's claim is indistinguishable from none.
	other := h.newOrg(t, "verify-other")
	if _, err := h.service.VerifyIdentityDomain(h.ctx, other, "admin-1", "admin", domain); !errors.Is(err, ErrDomainNotFound) {
		t.Fatalf("foreign claim verify: %v", err)
	}
	if err := h.service.RevokeIdentityDomain(h.ctx, other, "admin-1", "admin", domain); !errors.Is(err, ErrDomainNotFound) {
		t.Fatalf("foreign claim revoke: %v", err)
	}
	if _, err := h.service.LookupVerifiedDomainOwner(h.ctx, domain); !errors.Is(err, ErrDomainNotFound) {
		t.Fatalf("a pending claim is not a verified owner: %v", err)
	}
}

func TestIdentityDomainRevokeIsBlockedForTheLastDomainUnderEnforceAndReAddable(t *testing.T) {
	h := newIdentityHarness(t)
	org := h.newOrg(t, "revoke")
	h.addAndVerify(t, org, "revoke-one.example.com")
	if _, err := h.db.Pool.Exec(h.ctx, `
INSERT INTO organization_sso_policy (org_id, mode) VALUES ($1, 'enforce')
ON CONFLICT (org_id) DO UPDATE SET mode = 'enforce'`, org); err != nil {
		t.Fatal(err)
	}

	if err := h.service.RevokeIdentityDomain(h.ctx, org, "admin-1", "admin", "revoke-one.example.com"); !errors.Is(err, ErrDomainInUse) {
		t.Fatalf("last verified domain under enforce: %v, want ErrDomainInUse", err)
	}
	if n := h.outboxCount(t, org, IdentityEventDomainRevoked); n != 0 {
		t.Fatalf("a refused revoke must leave no audit row: %d", n)
	}

	// A pending claim never backs the policy, so it can go.
	if _, err := h.service.AddIdentityDomain(h.ctx, org, "admin-1", "admin", "revoke-pending.example.com"); err != nil {
		t.Fatal(err)
	}
	if err := h.service.RevokeIdentityDomain(h.ctx, org, "admin-1", "admin", "revoke-pending.example.com"); err != nil {
		t.Fatalf("revoke pending under enforce: %v", err)
	}

	// With a second verified domain the first may go.
	h.addAndVerify(t, org, "revoke-two.example.com")
	if err := h.service.RevokeIdentityDomain(h.ctx, org, "admin-1", "admin", "revoke-one.example.com"); err != nil {
		t.Fatalf("revoke with another verified domain: %v", err)
	}
	if err := h.service.RevokeIdentityDomain(h.ctx, org, "admin-1", "admin", "revoke-two.example.com"); !errors.Is(err, ErrDomainInUse) {
		t.Fatalf("the remaining domain is protected: %v", err)
	}

	// A revoked row is gone from the list, is not an owner, and can be claimed again.
	if err := h.service.RevokeIdentityDomain(h.ctx, org, "admin-1", "admin", "revoke-one.example.com"); !errors.Is(err, ErrDomainNotFound) {
		t.Fatalf("second revoke: %v", err)
	}
	if _, err := h.service.LookupVerifiedDomainOwner(h.ctx, "revoke-one.example.com"); !errors.Is(err, ErrDomainNotFound) {
		t.Fatalf("revoked owner lookup: %v", err)
	}
	again, err := h.service.AddIdentityDomain(h.ctx, org, "admin-1", "admin", "revoke-one.example.com")
	if err != nil || again.Status != "pending" || again.Challenge == nil || again.VerifiedAt != nil {
		t.Fatalf("re-add after revoke: %+v, %v", again, err)
	}
	if n := h.outboxCount(t, org, IdentityEventDomainRevoked); n != 2 {
		t.Fatalf("revoked audit rows: %d, want 2", n)
	}
}

func TestMigration028AppliesOnADatabaseHoldingALegacyAuthenticatedEmailRow(t *testing.T) {
	h := newIdentityHarness(t)
	org := h.newOrg(t, "legacy")
	if _, err := h.db.Pool.Exec(h.ctx, `
INSERT INTO organization_domains (org_id, normalized_domain, status, verification_method, auto_invite_enabled)
VALUES ($1, 'legacy-claim.example.com', 'pending', 'authenticated_email', false)`, org); err != nil {
		t.Fatalf("seed legacy pending row: %v", err)
	}
	sql, err := os.ReadFile("../../migrations/028_org_identity_domains.up.sql")
	if err != nil {
		t.Fatal(err)
	}
	// The migration is additive and idempotent: applying it again over that row must succeed.
	if _, err := h.db.Pool.Exec(h.ctx, string(sql)); err != nil {
		t.Fatalf("re-apply 028 over a legacy row: %v", err)
	}
	// The legacy claim has no challenge until an admin asks for one, then it gets one.
	listed, err := h.service.ListIdentityDomains(h.ctx, org)
	if err != nil || len(listed) != 1 || listed[0].Challenge != nil || listed[0].Status != "pending" {
		t.Fatalf("legacy row listing: %+v, %v", listed, err)
	}
	added, err := h.service.AddIdentityDomain(h.ctx, org, "admin-1", "admin", "legacy-claim.example.com")
	if err != nil || added.Challenge == nil {
		t.Fatalf("legacy row add: %+v, %v", added, err)
	}
	// And a legacy claim never counts as verified.
	if _, err := h.service.LookupVerifiedDomainOwner(h.ctx, "legacy-claim.example.com"); !errors.Is(err, ErrDomainNotFound) {
		t.Fatalf("legacy pending claim leaked as owner: %v", err)
	}
	// 'verified' without a proof method is refused by the table.
	if _, err := h.db.Pool.Exec(h.ctx, `
UPDATE organization_domains SET status = 'verified', verification_method = 'authenticated_email'
WHERE org_id = $1 AND normalized_domain = 'legacy-claim.example.com'`, org); err == nil {
		t.Fatal("authenticated_email must never be accepted as a verified method")
	}
}
