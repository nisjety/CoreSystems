package org

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"
)

// WS05-S7: SSO policy rules that need no database. The repository-backed rules (revision CAS,
// last provider, domain proof) are in sso_policy_postgres_test.go.

// fakeIdentityAdmin is the auth-core seam.
type fakeIdentityAdmin struct {
	providers    []SSOProviderRecord
	providersErr error
	signedIn     bool
	signInErr    error
	facts        []SSOMemberFact
	factsErr     error
	registerErr  error
	deleteErr    error

	registered     []SSOProviderRegistration
	deleted        []string
	revokeCalls    int
	revokedDomains []string
	revokeExempt   []string
	signInWithin   time.Duration
	factsAsked     []string
}

func (f *fakeIdentityAdmin) ListProviders(context.Context, string) ([]SSOProviderRecord, error) {
	return f.providers, f.providersErr
}

func (f *fakeIdentityAdmin) RegisterProvider(_ context.Context, req SSOProviderRegistration) error {
	f.registered = append(f.registered, req)
	return f.registerErr
}

func (f *fakeIdentityAdmin) DeleteProvider(_ context.Context, _, providerID, _ string) error {
	f.deleted = append(f.deleted, providerID)
	return f.deleteErr
}

func (f *fakeIdentityAdmin) HasRecentSignIn(_ context.Context, _, _ string, within time.Duration) (bool, error) {
	f.signInWithin = within
	return f.signedIn, f.signInErr
}

func (f *fakeIdentityAdmin) MemberFacts(_ context.Context, _ string, userIDs []string) ([]SSOMemberFact, error) {
	f.factsAsked = userIDs
	return f.facts, f.factsErr
}

func (f *fakeIdentityAdmin) RevokeSessions(_ context.Context, _, _ string, domains, exempt []string) (int, error) {
	f.revokeCalls++
	f.revokedDomains, f.revokeExempt = domains, exempt
	return 0, nil
}

func verifiedProvider() []SSOProvider {
	return []SSOProvider{{ProviderID: "acme-oidc", Domain: "example.com", DomainVerified: true}}
}

func ownerWithTwoFactor(id string) SSOMemberFact {
	return SSOMemberFact{UserID: id, Role: "owner", TwoFactorEnabled: true}
}

func reasonsFor(t *testing.T, admin *fakeIdentityAdmin, domains []string, providers []SSOProvider, providersErr error, breakGlass []string) []string {
	t.Helper()
	service := &Service{identityAdmin: admin}
	reasons, err := service.enforceReasons(context.Background(), "org-1", "actor-1", breakGlass, domains, providers, providersErr)
	if err != nil {
		t.Fatalf("enforceReasons: %v", err)
	}
	return reasons
}

func TestEnforcePreconditionsPassWhenEverythingHolds(t *testing.T) {
	admin := &fakeIdentityAdmin{signedIn: true, facts: []SSOMemberFact{ownerWithTwoFactor("owner-1")}}
	reasons := reasonsFor(t, admin, []string{"example.com"}, verifiedProvider(), nil, []string{"owner-1"})
	if len(reasons) != 0 {
		t.Fatalf("reasons = %v, want none", reasons)
	}
	if admin.signInWithin != SSOProofWindow || SSOProofWindow != 24*time.Hour {
		t.Fatalf("sign-in proof window = %v", admin.signInWithin)
	}
	if !equalStrings(admin.factsAsked, []string{"owner-1"}) {
		t.Fatalf("member facts asked for %v", admin.factsAsked)
	}
}

func TestEnforcePreconditionOrder(t *testing.T) {
	// Everything missing: the answer lists every blocker, in the order PUT reports them.
	admin := &fakeIdentityAdmin{}
	got := reasonsFor(t, admin, nil, nil, nil, nil)
	want := []string{SSOReasonNoVerifiedDomain, SSOReasonNoProvider, SSOReasonNotProven, SSOReasonBreakGlass}
	if !equalStrings(got, want) {
		t.Fatalf("reasons = %v, want %v", got, want)
	}

	cases := []struct {
		name       string
		admin      *fakeIdentityAdmin
		domains    []string
		providers  []SSOProvider
		breakGlass []string
		want       []string
	}{
		{"no verified domain only", &fakeIdentityAdmin{signedIn: true, facts: []SSOMemberFact{ownerWithTwoFactor("o")}}, nil, verifiedProvider(), []string{"o"}, []string{SSOReasonNoVerifiedDomain}},
		{"provider on an unverified domain is no provider", &fakeIdentityAdmin{signedIn: true, facts: []SSOMemberFact{ownerWithTwoFactor("o")}}, []string{"example.com"}, []SSOProvider{{ProviderID: "x", DomainVerified: false}}, []string{"o"}, []string{SSOReasonNoProvider}},
		{"own sign-in not proven", &fakeIdentityAdmin{signedIn: false, facts: []SSOMemberFact{ownerWithTwoFactor("o")}}, []string{"example.com"}, verifiedProvider(), []string{"o"}, []string{SSOReasonNotProven}},
		{"break glass unknown user", &fakeIdentityAdmin{signedIn: true}, []string{"example.com"}, verifiedProvider(), []string{"ghost"}, []string{SSOReasonBreakGlass}},
		{"break glass not an owner", &fakeIdentityAdmin{signedIn: true, facts: []SSOMemberFact{{UserID: "a", Role: "admin", TwoFactorEnabled: true}}}, []string{"example.com"}, verifiedProvider(), []string{"a"}, []string{SSOReasonBreakGlass}},
		{"break glass owner without 2fa", &fakeIdentityAdmin{signedIn: true, facts: []SSOMemberFact{{UserID: "o", Role: "owner"}}}, []string{"example.com"}, verifiedProvider(), []string{"o"}, []string{SSOReasonBreakGlass2FA}},
		{"owner role is case insensitive", &fakeIdentityAdmin{signedIn: true, facts: []SSOMemberFact{{UserID: "o", Role: "Owner", TwoFactorEnabled: true}}}, []string{"example.com"}, verifiedProvider(), []string{"o"}, []string{}},
		{"one good owner and one bad", &fakeIdentityAdmin{signedIn: true, facts: []SSOMemberFact{ownerWithTwoFactor("o1"), {UserID: "o2", Role: "member", TwoFactorEnabled: true}}}, []string{"example.com"}, verifiedProvider(), []string{"o1", "o2"}, []string{SSOReasonBreakGlass}},
		{"invalid and no 2fa together", &fakeIdentityAdmin{signedIn: true, facts: []SSOMemberFact{{UserID: "o1", Role: "owner"}, {UserID: "m", Role: "member"}}}, []string{"example.com"}, verifiedProvider(), []string{"o1", "m"}, []string{SSOReasonBreakGlass, SSOReasonBreakGlass2FA}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := reasonsFor(t, tc.admin, tc.domains, tc.providers, nil, tc.breakGlass)
			if !equalStrings(got, tc.want) {
				t.Fatalf("reasons = %v, want %v", got, tc.want)
			}
		})
	}
}

// An auth-core failure is a reason, never a pass: enforce must not be allowed on a guess.
func TestEnforcePreconditionsTreatAProviderOutageAsABlocker(t *testing.T) {
	down := errors.New("auth-core down")
	provider := verifiedProvider()
	cases := map[string]struct {
		admin        *fakeIdentityAdmin
		providers    []SSOProvider
		providersErr error
		want         []string
	}{
		"provider list failed": {&fakeIdentityAdmin{}, nil, down, []string{SSOReasonNoVerifiedDomain, ssoReasonProviderDown}},
		"sign-in check failed": {&fakeIdentityAdmin{signInErr: down}, provider, nil, []string{ssoReasonProviderDown}},
		"member facts failed":  {&fakeIdentityAdmin{signedIn: true, factsErr: down}, provider, nil, []string{ssoReasonProviderDown}},
	}
	for name, tc := range cases {
		domains := []string{"example.com"}
		if name == "provider list failed" {
			domains = nil
		}
		got := reasonsFor(t, tc.admin, domains, tc.providers, tc.providersErr, []string{"o"})
		if !equalStrings(got, tc.want) {
			t.Fatalf("%s: reasons = %v, want %v", name, got, tc.want)
		}
	}
}

func TestNormalizeBreakGlass(t *testing.T) {
	got, err := normalizeBreakGlass([]string{" owner-1 ", "owner-2"})
	if err != nil || !equalStrings(got, []string{"owner-1", "owner-2"}) {
		t.Fatalf("trim: %v, %v", got, err)
	}
	if got, err := normalizeBreakGlass(nil); err != nil || got == nil || len(got) != 0 {
		t.Fatalf("empty list must be a non-nil empty slice: %v, %v", got, err)
	}
	for name, ids := range map[string][]string{
		"blank":        {" "},
		"duplicate":    {"a", "a"},
		"trimmed dup":  {"a", " a "},
		"three":        {"a", "b", "c"},
		"over 128":     {strings.Repeat("x", 129)},
		"blank in two": {"a", ""},
	} {
		_, err := normalizeBreakGlass(ids)
		var bad *SSORequestError
		if !errors.As(err, &bad) || bad.Reason != "break_glass_user_ids" {
			t.Fatalf("%s: err = %v, want SSORequestError break_glass_user_ids", name, err)
		}
	}
	if MaxSSOBreakGlass != 2 {
		t.Fatalf("MaxSSOBreakGlass = %d", MaxSSOBreakGlass)
	}
}

func TestValidateSSOIssuer(t *testing.T) {
	for _, ok := range []string{
		"https://idp.example.com",
		"https://idp.example.com/realms/acme",
		"https://login.example.com:8443/oidc",
	} {
		if err := validateSSOIssuer(ok); err != nil {
			t.Errorf("%q refused: %v", ok, err)
		}
	}
	for name, bad := range map[string]string{
		"empty":        "",
		"http":         "http://idp.example.com",
		"no scheme":    "idp.example.com",
		"ftp":          "ftp://idp.example.com",
		"no host":      "https://",
		"userinfo":     "https://user:pw@idp.example.com",
		"query":        "https://idp.example.com/?tenant=1",
		"fragment":     "https://idp.example.com/#x",
		"javascript":   "javascript:alert(1)",
		"garbage":      "not a url at all",
		"over 2048":    "https://idp.example.com/" + strings.Repeat("a", 2100),
		"control char": "https://idp.example.com/\x00",
	} {
		if err := validateSSOIssuer(bad); !errors.Is(err, ErrSSOInvalidIssuer) {
			t.Errorf("%s (%q): err = %v, want ErrSSOInvalidIssuer", name, bad, err)
		}
	}
}

func TestSSOModeValidity(t *testing.T) {
	for _, mode := range []string{"off", "report", "enforce"} {
		if !validSSOMode(mode) {
			t.Errorf("%q must be valid", mode)
		}
	}
	for _, mode := range []string{"", "Enforce", "ON", "enforced", " off", "block"} {
		if validSSOMode(mode) {
			t.Errorf("%q must be invalid", mode)
		}
	}
}

func TestSSOServiceRefusesBeforeTheRepository(t *testing.T) {
	ctx := context.Background()

	// Feature off: everything is ErrDomainVerificationUnavailable.
	off := &Service{}
	if _, err := off.PutSSOPolicy(ctx, "org-1", "u", "admin", SSOPolicyInput{Mode: "report"}); !errors.Is(err, ErrDomainVerificationUnavailable) {
		t.Fatalf("put with the feature off: %v", err)
	}
	if _, err := off.GetSSOPolicy(ctx, "org-1", "u"); !errors.Is(err, ErrDomainVerificationUnavailable) {
		t.Fatalf("get with the feature off: %v", err)
	}
	if _, err := off.LookupSSOPolicyByDomain(ctx, "example.com"); !errors.Is(err, ErrDomainVerificationUnavailable) {
		t.Fatalf("by-domain with the feature off: %v", err)
	}
	if _, err := off.AddSSOProvider(ctx, "org-1", "u", "admin", SSOProviderInput{}); !errors.Is(err, ErrDomainVerificationUnavailable) {
		t.Fatalf("add provider with the feature off: %v", err)
	}
	if err := off.RemoveSSOProvider(ctx, "org-1", "u", "admin", "acme-oidc"); !errors.Is(err, ErrDomainVerificationUnavailable) {
		t.Fatalf("remove provider with the feature off: %v", err)
	}

	// Feature on, no auth-core client: provider administration is unavailable.
	noAdmin := &Service{}
	noAdmin.EnableDomainVerification(&fakeTXTResolver{})
	if _, err := noAdmin.AddSSOProvider(ctx, "org-1", "u", "admin", SSOProviderInput{ProviderID: "acme-oidc"}); !errors.Is(err, ErrSSOUnavailable) {
		t.Fatalf("add provider without auth-core: %v", err)
	}
	if err := noAdmin.RemoveSSOProvider(ctx, "org-1", "u", "admin", "acme-oidc"); !errors.Is(err, ErrSSOUnavailable) {
		t.Fatalf("remove provider without auth-core: %v", err)
	}
}

func TestSSOPutPolicyValidatesTheRequestBeforeTheRepository(t *testing.T) {
	service := &Service{}
	service.EnableDomainVerification(&fakeTXTResolver{})
	service.SetIdentityProviderAdmin(&fakeIdentityAdmin{})
	cases := map[string]SSOPolicyInput{
		"empty mode":        {Mode: ""},
		"unknown mode":      {Mode: "block"},
		"upper-case mode":   {Mode: "ENFORCE"},
		"negative revision": {Mode: "report", ExpectedRevision: -1},
	}
	for name, in := range cases {
		_, err := service.PutSSOPolicy(context.Background(), "org-1", "u", "admin", in)
		var bad *SSORequestError
		if !errors.As(err, &bad) || bad.Reason != "mode" {
			t.Fatalf("%s: err = %v, want SSORequestError mode", name, err)
		}
	}
	_, err := service.PutSSOPolicy(context.Background(), "org-1", "u", "admin", SSOPolicyInput{Mode: "report", BreakGlassUserIDs: []string{"a", "a"}})
	var bad *SSORequestError
	if !errors.As(err, &bad) || bad.Reason != "break_glass_user_ids" {
		t.Fatalf("duplicate break glass: %v", err)
	}
}

func TestSSOAddProviderValidatesBeforeAnyCallOut(t *testing.T) {
	admin := &fakeIdentityAdmin{}
	service := &Service{}
	service.EnableDomainVerification(&fakeTXTResolver{})
	service.SetIdentityProviderAdmin(admin)
	secret := "sssssssssssssssssssssssssss"
	good := SSOProviderInput{ProviderID: "acme-oidc", Domain: "example.com", Type: "oidc", Issuer: "https://idp.example.com", ClientID: "client-1", ClientSecret: secret}
	mutate := func(edit func(*SSOProviderInput)) SSOProviderInput {
		in := good
		edit(&in)
		return in
	}
	cases := []struct {
		name   string
		in     SSOProviderInput
		reason string // SSORequestError reason, or "" when a sentinel is expected
		is     error
	}{
		{"id too short", mutate(func(i *SSOProviderInput) { i.ProviderID = "ab" }), "provider_id", nil},
		{"id with space", mutate(func(i *SSOProviderInput) { i.ProviderID = "acme oidc" }), "provider_id", nil},
		{"id leading hyphen", mutate(func(i *SSOProviderInput) { i.ProviderID = "-acme" }), "provider_id", nil},
		{"id underscore", mutate(func(i *SSOProviderInput) { i.ProviderID = "acme_oidc" }), "provider_id", nil},
		{"id too long", mutate(func(i *SSOProviderInput) { i.ProviderID = strings.Repeat("a", 64) }), "provider_id", nil},
		{"id empty", mutate(func(i *SSOProviderInput) { i.ProviderID = "" }), "provider_id", nil},
		{"type saml", mutate(func(i *SSOProviderInput) { i.Type = "saml" }), "type", nil},
		{"type empty", mutate(func(i *SSOProviderInput) { i.Type = "" }), "type", nil},
		{"client id empty", mutate(func(i *SSOProviderInput) { i.ClientID = " " }), "client_credentials", nil},
		{"client secret empty", mutate(func(i *SSOProviderInput) { i.ClientSecret = "" }), "client_credentials", nil},
		{"client secret too long", mutate(func(i *SSOProviderInput) { i.ClientSecret = strings.Repeat("s", 513) }), "client_credentials", nil},
		{"issuer http", mutate(func(i *SSOProviderInput) { i.Issuer = "http://idp.example.com" }), "", ErrSSOInvalidIssuer},
		{"issuer empty", mutate(func(i *SSOProviderInput) { i.Issuer = "" }), "", ErrSSOInvalidIssuer},
		{"issuer userinfo", mutate(func(i *SSOProviderInput) { i.Issuer = "https://a:b@idp.example.com" }), "", ErrSSOInvalidIssuer},
		{"free mail domain is not verified", mutate(func(i *SSOProviderInput) { i.Domain = "gmail.com" }), "", ErrSSODomainNotVerified},
		{"ip domain is not verified", mutate(func(i *SSOProviderInput) { i.Domain = "10.0.0.1" }), "", ErrSSODomainNotVerified},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			_, err := service.AddSSOProvider(context.Background(), "org-1", "u", "admin", tc.in)
			if tc.is != nil {
				if !errors.Is(err, tc.is) {
					t.Fatalf("err = %v, want %v", err, tc.is)
				}
			} else {
				var bad *SSORequestError
				if !errors.As(err, &bad) || bad.Reason != tc.reason {
					t.Fatalf("err = %v, want SSORequestError %s", err, tc.reason)
				}
			}
			if err != nil && strings.Contains(err.Error(), secret) {
				t.Fatalf("the error carries the client secret: %v", err)
			}
			if len(admin.registered) != 0 {
				t.Fatal("a refused request must not reach auth-core")
			}
		})
	}
}

func TestSSORemoveProviderValidatesTheIDBeforeTheRepository(t *testing.T) {
	service := &Service{}
	service.EnableDomainVerification(&fakeTXTResolver{})
	admin := &fakeIdentityAdmin{}
	service.SetIdentityProviderAdmin(admin)
	for _, id := range []string{"", "ab", "-abc", "acme oidc", "a/b", strings.Repeat("a", 64), "../etc"} {
		err := service.RemoveSSOProvider(context.Background(), "org-1", "u", "admin", id)
		var bad *SSORequestError
		if !errors.As(err, &bad) || bad.Reason != "provider_id" {
			t.Fatalf("id %q: err = %v, want SSORequestError provider_id", id, err)
		}
	}
	if len(admin.deleted) != 0 {
		t.Fatal("a refused id must not reach auth-core")
	}
}

func TestSSOErrorTypesCarryTheirCode(t *testing.T) {
	if got := (&SSOEnforceError{Code: SSOReasonNoProvider}).Error(); !strings.Contains(got, SSOReasonNoProvider) {
		t.Fatalf("enforce error text %q", got)
	}
	if got := (&SSORequestError{Reason: "mode"}).Error(); !strings.Contains(got, "mode") {
		t.Fatalf("request error text %q", got)
	}
	// The closed reason strings are API surface (typed 422 codes); pin them.
	for got, want := range map[string]string{
		SSOReasonNoVerifiedDomain: "no_verified_domain",
		SSOReasonNoProvider:       "no_provider",
		SSOReasonNotProven:        "sso_not_proven",
		SSOReasonBreakGlass:       "break_glass_invalid",
		SSOReasonBreakGlass2FA:    "break_glass_requires_2fa",
	} {
		if got != want {
			t.Fatalf("reason constant %q changed, want %q", got, want)
		}
	}
}
