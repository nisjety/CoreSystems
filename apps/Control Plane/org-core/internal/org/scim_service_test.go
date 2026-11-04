package org

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"
)

// WS05-S8: SCIM Users in org-core. Pure logic, no database. The paths that read or write
// organization_scim_* (fake-provisioner flows, tokens, RLS, migration 029) are in
// scim_postgres_test.go, gated on CONTROL_LIFECYCLE_TEST_DATABASE_URL.

func TestSCIMOperationIDIsStablePerTransitionAndAValidV5UUID(t *testing.T) {
	scimID := "11111111-2222-3333-4444-555555555555"
	a := scimOperationID("org-1", scimID, false, 3)
	b := scimOperationID("org-1", scimID, false, 3)
	if a != b {
		t.Fatalf("the same transition must give the same operation id: %s vs %s", a, b)
	}
	if !validUUID(a) {
		t.Fatalf("operation id %q is not a UUID", a)
	}
	// Version nibble 5, RFC 4122 variant (8, 9, a or b).
	if a[14] != '5' {
		t.Fatalf("operation id %q is not version 5", a)
	}
	if !strings.ContainsRune("89ab", rune(a[19])) {
		t.Fatalf("operation id %q is not the RFC 4122 variant", a)
	}
	variants := map[string]string{
		"org":      scimOperationID("org-2", scimID, false, 3),
		"scim id":  scimOperationID("org-1", "11111111-2222-3333-4444-555555555556", false, 3),
		"target":   scimOperationID("org-1", scimID, true, 3),
		"revision": scimOperationID("org-1", scimID, false, 4),
	}
	seen := map[string]string{a: "base"}
	for name, id := range variants {
		if prev, dup := seen[id]; dup {
			t.Fatalf("operation id for %s collides with %s", name, prev)
		}
		seen[id] = name
	}
}

func TestSCIMFormatUUIDSetsVersionAndVariant(t *testing.T) {
	for _, version := range []byte{4, 5} {
		got := formatUUID(make([]byte, 16), version)
		if got[14] != '0'+version {
			t.Fatalf("version %d: %q", version, got)
		}
		if !validUUID(got) {
			t.Fatalf("not a UUID: %q", got)
		}
	}
	fresh, err := newUUID()
	if err != nil || !validUUID(fresh) || fresh[14] != '4' {
		t.Fatalf("newUUID() = %q, %v", fresh, err)
	}
	other, _ := newUUID()
	if fresh == other {
		t.Fatal("newUUID repeated itself")
	}
}

func TestSCIMValidUUIDIsCaseInsensitiveAndStrict(t *testing.T) {
	good := []string{
		"11111111-2222-3333-4444-555555555555",
		"AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE",
	}
	for _, id := range good {
		if !validUUID(id) {
			t.Fatalf("%q must be valid", id)
		}
	}
	bad := []string{"", "x", "1111111-2222-3333-4444-555555555555", "11111111-2222-3333-4444-55555555555g",
		"11111111222233334444555555555555", "11111111-2222-3333-4444-555555555555 ", "../etc/passwd"}
	for _, id := range bad {
		if validUUID(id) {
			t.Fatalf("%q must be refused", id)
		}
	}
}

func TestNormalizeSCIMUserName(t *testing.T) {
	cases := []struct {
		name       string
		in         string
		wantEmail  string
		wantDomain string
		refused    bool
	}{
		{name: "plain", in: "ada@corp.example.com", wantEmail: "ada@corp.example.com", wantDomain: "corp.example.com"},
		{name: "uppercase", in: "Ada.Lovelace@Corp.EXAMPLE.com", wantEmail: "ada.lovelace@corp.example.com", wantDomain: "corp.example.com"},
		{name: "surrounding space", in: "  ada@corp.example.com\t", wantEmail: "ada@corp.example.com", wantDomain: "corp.example.com"},
		{name: "plus tag kept", in: "ada+idp@corp.example.com", wantEmail: "ada+idp@corp.example.com", wantDomain: "corp.example.com"},
		{name: "idn domain to punycode", in: "ada@bücher.example", wantEmail: "ada@bücher.example", wantDomain: "xn--bcher-kva.example"},

		{name: "empty", in: "", refused: true},
		{name: "blank", in: "   ", refused: true},
		{name: "no at", in: "ada.corp.example.com", refused: true},
		{name: "two ats", in: "a@b@corp.example.com", refused: true},
		{name: "empty local part", in: "@corp.example.com", refused: true},
		{name: "empty domain", in: "ada@", refused: true},
		{name: "inner space", in: "ada lovelace@corp.example.com", refused: true},
		{name: "local part over 64", in: strings.Repeat("a", 65) + "@corp.example.com", refused: true},
		{name: "over 254", in: "a@" + strings.Repeat("b", 63) + "." + strings.Repeat("c", 63) + "." + strings.Repeat("d", 63) + "." + strings.Repeat("e", 63) + ".com", refused: true},
		{name: "free mail domain", in: "ada@gmail.com", refused: true},
		{name: "ip literal domain", in: "ada@192.168.0.1", refused: true},
		{name: "single label domain", in: "ada@localhost", refused: true},
		{name: "public suffix domain", in: "ada@co.uk", refused: true},
		{name: "scheme in domain", in: "ada@https://corp.example.com", refused: true},
		{name: "trailing dot domain", in: "ada@corp.example.com.", refused: true},
		{name: "two trailing dots", in: "ada@corp.example.com..", refused: true},
		{name: "single letter tld", in: "ada@corp.example.c", refused: true},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			email, domain, err := NormalizeSCIMUserName(tc.in)
			if tc.refused {
				var reqErr *SCIMRequestError
				if !errors.As(err, &reqErr) || reqErr.Reason != "userName" {
					t.Fatalf("NormalizeSCIMUserName(%q) = %q, %q, %v; want a SCIMRequestError userName", tc.in, email, domain, err)
				}
				if email != "" || domain != "" {
					t.Fatalf("a refusal must return no values, got %q %q", email, domain)
				}
				return
			}
			if err != nil {
				t.Fatalf("NormalizeSCIMUserName(%q) refused: %v", tc.in, err)
			}
			if domain != tc.wantDomain {
				t.Fatalf("domain = %q, want %q", domain, tc.wantDomain)
			}
			if strings.ToLower(strings.TrimSpace(tc.in)) != email {
				t.Fatalf("email = %q, want the lowercased trimmed input", email)
			}
		})
	}
}

func TestSCIMRequestErrorNamesItsReason(t *testing.T) {
	err := error(&SCIMRequestError{Reason: "externalId"})
	if !strings.Contains(err.Error(), "externalId") {
		t.Fatalf("error text: %q", err.Error())
	}
}

func TestSCIMTokenHashAndGeneration(t *testing.T) {
	h := hashSCIMToken("scim_abc")
	if len(h) != 64 || strings.Trim(h, "0123456789abcdef") != "" {
		t.Fatalf("hash is not 64 lowercase hex characters: %q", h)
	}
	if h != hashSCIMToken("scim_abc") || h == hashSCIMToken("scim_abd") {
		t.Fatal("hash must be deterministic and input-sensitive")
	}
	if strings.Contains(h, "scim_abc") {
		t.Fatal("hash leaks the token")
	}

	tok, err := newSCIMToken()
	if err != nil {
		t.Fatal(err)
	}
	if !strings.HasPrefix(tok, SCIMTokenPrefix) {
		t.Fatalf("token %q lacks the %q prefix", tok, SCIMTokenPrefix)
	}
	// 32 random bytes are 43 base64url characters; the whole token stays inside the resolver's 128 limit.
	if len(tok) != len(SCIMTokenPrefix)+43 {
		t.Fatalf("token length %d, want %d", len(tok), len(SCIMTokenPrefix)+43)
	}
	if strings.ContainsAny(tok[len(SCIMTokenPrefix):], "+/=") {
		t.Fatalf("token body must be base64url without padding: %q", tok)
	}
	again, _ := newSCIMToken()
	if tok == again {
		t.Fatal("newSCIMToken repeated itself")
	}
	if MaxActiveSCIMTokens != 3 {
		t.Fatalf("MaxActiveSCIMTokens = %d, the plan says 3", MaxActiveSCIMTokens)
	}
}

func TestSCIMAuditEventsAreRegisteredWithSingleTokenSubjects(t *testing.T) {
	events := map[string]string{
		IdentityEventSCIMTokenCreated:      "scim_token",
		IdentityEventSCIMTokenRevoked:      "scim_token",
		IdentityEventSCIMUserProvisioned:   "scim_user",
		IdentityEventSCIMUserDeprovisioned: "scim_user",
		IdentityEventSCIMUserReactivated:   "scim_user",
		IdentityEventSCIMRefusedLastOwner:  "scim_user",
	}
	at := time.Date(2026, 11, 1, 9, 0, 0, 0, time.UTC)
	for event, kind := range events {
		if _, ok := identityAuditEvents[event]; !ok {
			t.Fatalf("%s is not registered in identityAuditEvents; enqueueIdentityAudit would refuse it", event)
		}
		if strings.Contains(event, ".") {
			t.Fatalf("%s: audit-core accepts only single-token event names", event)
		}
		_, subject, payload := identityAuditPayload(identityAuditChange{
			Event: event, OrgID: "org-1", ActorID: "user-1", ActorRole: scimActorRole, Resource: "res-1",
		}, at)
		if got := len(strings.Split(subject, ".")); got != 6 {
			t.Fatalf("%s: subject %q has %d tokens, want 6", event, subject, got)
		}
		if payload["subject"] != kind+":res-1" {
			t.Fatalf("%s: payload subject = %v, want %s:res-1", event, payload["subject"], kind)
		}
		if payload["actor_role"] != "scim" {
			t.Fatalf("%s: actor role = %v", event, payload["actor_role"])
		}
	}
}

// --- the switch-off behaviour and request validation that precede any database access ---

func newScimOffService() *Service { return &Service{} }

type neverCalledProvisioner struct{ calls int }

func (p *neverCalledProvisioner) ProvisionMember(context.Context, SCIMProvisionRequest) (*SCIMProvisionResult, error) {
	p.calls++
	return nil, errors.New("must not be called")
}

func (p *neverCalledProvisioner) DeprovisionMember(context.Context, SCIMDeprovisionRequest) (*SCIMDeprovisionResult, error) {
	p.calls++
	return nil, errors.New("must not be called")
}

func TestSCIMEveryMethodAnswersUnavailableWithoutAProvisioner(t *testing.T) {
	ctx := context.Background()
	s := newScimOffService()
	if s.SCIMEnabled() {
		t.Fatal("SCIM must default to disabled")
	}
	auth := &SCIMTokenAuth{TokenID: "t", OrgID: "org-1", CreatedBy: "admin-1"}

	checks := map[string]error{}
	_, checks["CreateSCIMToken"] = s.CreateSCIMToken(ctx, "org-1", "admin-1", "admin", "idp", 30)
	_, checks["ListSCIMTokens"] = s.ListSCIMTokens(ctx, "org-1")
	checks["RevokeSCIMToken"] = s.RevokeSCIMToken(ctx, "org-1", "admin-1", "admin", "11111111-2222-3333-4444-555555555555")
	_, checks["GetSCIMStatus"] = s.GetSCIMStatus(ctx, "org-1")
	_, checks["ResolveSCIMToken"] = s.ResolveSCIMToken(ctx, "scim_whatever")
	_, _, checks["ListSCIMUsers"] = s.ListSCIMUsers(ctx, auth, nil, 1, 10)
	_, checks["GetSCIMUser"] = s.GetSCIMUser(ctx, auth, "11111111-2222-3333-4444-555555555555")
	_, checks["CreateSCIMUser"] = s.CreateSCIMUser(ctx, auth, SCIMUserInput{UserName: "ada@corp.example.com"})
	_, checks["SetSCIMUserActive"] = s.SetSCIMUserActive(ctx, auth, "11111111-2222-3333-4444-555555555555", false)
	checks["DeleteSCIMUser"] = s.DeleteSCIMUser(ctx, auth, "11111111-2222-3333-4444-555555555555")

	for name, err := range checks {
		if !errors.Is(err, ErrSCIMUnavailable) {
			t.Fatalf("%s without a provisioner = %v, want ErrSCIMUnavailable", name, err)
		}
	}

	p := &neverCalledProvisioner{}
	s.SetScimProvisioner(p)
	if !s.SCIMEnabled() {
		t.Fatal("SetScimProvisioner must enable SCIM")
	}
}

func TestSCIMCreateTokenValidatesNameAndLifetimeBeforeTouchingTheDatabase(t *testing.T) {
	s := newScimOffService()
	s.SetScimProvisioner(&neverCalledProvisioner{})
	ctx := context.Background()

	cases := []struct {
		name   string
		in     string
		days   int
		reason string
	}{
		{name: "empty name", in: "", days: 30, reason: "name"},
		{name: "blank name", in: "   ", days: 30, reason: "name"},
		{name: "name over 80", in: strings.Repeat("n", 81), days: 30, reason: "name"},
		{name: "zero days", in: "Entra", days: 0, reason: "expires_in_days"},
		{name: "negative days", in: "Entra", days: -1, reason: "expires_in_days"},
		{name: "366 days", in: "Entra", days: 366, reason: "expires_in_days"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			issued, err := s.CreateSCIMToken(ctx, "org-1", "admin-1", "admin", tc.in, tc.days)
			var reqErr *SCIMRequestError
			if !errors.As(err, &reqErr) || reqErr.Reason != tc.reason {
				t.Fatalf("CreateSCIMToken = %+v, %v; want SCIMRequestError %s", issued, err, tc.reason)
			}
			if issued != nil {
				t.Fatal("a refused request must not return a token")
			}
		})
	}
}

func TestSCIMTokenIDsAndSCIMIDsThatAreNotUUIDsAreNotFoundWithoutADatabase(t *testing.T) {
	s := newScimOffService()
	s.SetScimProvisioner(&neverCalledProvisioner{})
	ctx := context.Background()
	auth := &SCIMTokenAuth{TokenID: "t", OrgID: "org-1", CreatedBy: "admin-1"}

	if err := s.RevokeSCIMToken(ctx, "org-1", "admin-1", "admin", "not-a-uuid"); !errors.Is(err, ErrSCIMTokenNotFound) {
		t.Fatalf("revoke with a bad id = %v", err)
	}
	if _, err := s.GetSCIMUser(ctx, auth, "../../etc"); !errors.Is(err, ErrSCIMUserNotFound) {
		t.Fatalf("get with a bad id = %v", err)
	}
	if _, err := s.SetSCIMUserActive(ctx, auth, "nope", true); !errors.Is(err, ErrSCIMUserNotFound) {
		t.Fatalf("patch with a bad id = %v", err)
	}
	// DELETE on an unknown id succeeds (RFC 7644 clients retry deletes).
	if err := s.DeleteSCIMUser(ctx, auth, "nope"); err != nil {
		t.Fatalf("delete with a bad id = %v, want nil", err)
	}
}

func TestSCIMResolveTokenRefusesWrongShapesBeforeTheDatabase(t *testing.T) {
	s := newScimOffService()
	s.SetScimProvisioner(&neverCalledProvisioner{})
	for _, bearer := range []string{"", "   ", "Bearer scim_x", "tok_abcdef", "scim", strings.Repeat("scim_", 40)} {
		if _, err := s.ResolveSCIMToken(context.Background(), bearer); !errors.Is(err, ErrSCIMUnauthorized) {
			t.Fatalf("ResolveSCIMToken(%q) = %v, want ErrSCIMUnauthorized", bearer, err)
		}
	}
}

func TestSCIMCreateUserValidatesBeforeTouchingTheDatabaseOrAuthCore(t *testing.T) {
	p := &neverCalledProvisioner{}
	s := newScimOffService()
	s.SetScimProvisioner(p)
	auth := &SCIMTokenAuth{TokenID: "t", OrgID: "org-1", CreatedBy: "admin-1"}

	_, err := s.CreateSCIMUser(context.Background(), auth, SCIMUserInput{UserName: "not an email"})
	var reqErr *SCIMRequestError
	if !errors.As(err, &reqErr) || reqErr.Reason != "userName" {
		t.Fatalf("bad userName = %v", err)
	}
	_, err = s.CreateSCIMUser(context.Background(), auth, SCIMUserInput{UserName: "ada@corp.example.com", ExternalID: strings.Repeat("x", 256)})
	if !errors.As(err, &reqErr) || reqErr.Reason != "externalId" {
		t.Fatalf("long externalId = %v", err)
	}
	if p.calls != 0 {
		t.Fatalf("auth-core was called %d times for a refused request", p.calls)
	}
}

func TestSCIMListUsersRefusesFiltersOtherThanUserNameAndExternalId(t *testing.T) {
	s := newScimOffService()
	s.SetScimProvisioner(&neverCalledProvisioner{})
	auth := &SCIMTokenAuth{OrgID: "org-1"}
	for _, attr := range []string{"", "displayName", "emails.value", "id", "username"} {
		_, _, err := s.ListSCIMUsers(context.Background(), auth, &SCIMFilter{Attribute: attr, Value: "x"}, 1, 10)
		if !errors.Is(err, ErrSCIMInvalidFilter) {
			t.Fatalf("filter on %q = %v, want ErrSCIMInvalidFilter", attr, err)
		}
	}
}
