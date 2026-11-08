package server

import (
	"context"
	"strings"
	"testing"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
	"github.com/triodelab/model-plane/services/browser-broker/internal/sitepolicy"
	"google.golang.org/genproto/googleapis/rpc/errdetails"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// E3.2-e1 (OD13): the broker side of origin-extension grants. Everything is
// dark behind BROWSER_BROKER_ORIGIN_EXTENSION_ENABLED.

func extServer(store grant.Repository, principal authctx.Principal, enabled bool) *Server {
	cfg := DefaultConfig()
	cfg.OriginExtensionEnabled = enabled
	srv := NewServerWithConfig(store, cfg)
	srv.principal = func(context.Context) (authctx.Principal, error) { return principal, nil }
	return srv
}

// extParent issues a session grant and a run grant under it for the user
// (org1 / user1) and returns the run grant's id and session key.
func extParent(t *testing.T, store grant.Repository, principal authctx.Principal) (id, sessionKey string) {
	t.Helper()
	srv := extServer(store, principal, true)
	session, err := srv.AcquireGrant(context.Background(), &AcquireGrantRequest{
		OrgId: "org1", SessionKey: "s1", Mode: "cloud", AllowedDomains: []string{"example.com"},
	})
	if err != nil {
		t.Fatal(err)
	}
	run, err := srv.DeriveRunGrant(context.Background(), deriveReq(session.GetGrantId()))
	if err != nil {
		t.Fatal(err)
	}
	return run.GetGrantId(), "browser-01J"
}

func extReq(parent, sessionKey string, mutate ...func(*AcquireGrantRequest)) *AcquireGrantRequest {
	req := &AcquireGrantRequest{
		OrgId: "org1", SessionKey: sessionKey, Mode: "cloud", GrantKind: "origin_extension",
		ParentGrantId: parent, AllowedDomains: []string{"docs.other.org"}, ConsentScope: "turn",
	}
	for _, m := range mutate {
		m(req)
	}
	return req
}

func errorReason(err error) string {
	for _, detail := range status.Convert(err).Details() {
		if info, ok := detail.(*errdetails.ErrorInfo); ok {
			return info.GetReason()
		}
	}
	return ""
}

func TestOriginExtension_DefaultConfigIsOff(t *testing.T) {
	if DefaultConfig().OriginExtensionEnabled {
		t.Fatal("origin extensions must default off")
	}
	cfg, err := ConfigFromEnv(envOf(map[string]string{}))
	if err != nil || cfg.OriginExtensionEnabled {
		t.Fatalf("unset switch: enabled=%v err=%v", cfg.OriginExtensionEnabled, err)
	}
	for raw, want := range map[string]bool{
		"1": true, "true": true, "on": true, "TRUE": true, " on ": true,
		"0": false, "false": false, "off": false, "yes": false, "2": false, "tru": false, "": false,
	} {
		cfg, err := ConfigFromEnv(envOf(map[string]string{EnvOriginExtensionEnabled: raw}))
		if err != nil || cfg.OriginExtensionEnabled != want {
			t.Fatalf("%q: enabled=%v err=%v, want %v", raw, cfg.OriginExtensionEnabled, err, want)
		}
	}
	if EnvOriginExtensionEnabled != "BROWSER_BROKER_ORIGIN_EXTENSION_ENABLED" {
		t.Fatalf("switch name = %q", EnvOriginExtensionEnabled)
	}
}

func TestOriginExtension_SwitchOffRefusesAndValidatesAsRevoked(t *testing.T) {
	store := grant.NewMemoryRepository()
	user := userPrincipal(false)
	parent, key := extParent(t, store, user)
	off := extServer(store, user, false)
	_, err := off.AcquireGrant(context.Background(), extReq(parent, key))
	if status.Code(err) != codes.PermissionDenied || errorReason(err) != "ORIGIN_EXTENSION_DISABLED" {
		t.Fatalf("switch off: code=%v reason=%q (%v)", status.Code(err), errorReason(err), err)
	}
	// Issue one with the switch on, then turn it off: it validates as revoked.
	on := extServer(store, user, true)
	resp, err := on.AcquireGrant(context.Background(), extReq(parent, key))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := on.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: resp.GetGrantId()}); err != nil {
		t.Fatalf("validate with the switch on: %v", err)
	}
	if _, err := off.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: resp.GetGrantId()}); status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("validate with the switch off: code = %v, want FailedPrecondition", status.Code(err))
	}
	// The parent run grant is unaffected.
	if _, err := off.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: parent}); err != nil {
		t.Fatalf("the parent run grant must still validate: %v", err)
	}
}

func TestOriginExtension_IssuesAParentBoundGrantAndValidatesIt(t *testing.T) {
	store := grant.NewMemoryRepository()
	user := userPrincipal(false)
	parent, key := extParent(t, store, user)
	srv := extServer(store, user, true)
	resp, err := srv.AcquireGrant(context.Background(), extReq(parent, key, func(r *AcquireGrantRequest) {
		r.ConsentScope = "conversation"
		r.AllowedDomains = []string{"Docs.Other.org", "api.other.org"}
	}))
	if err != nil {
		t.Fatal(err)
	}
	if resp.GetKind() != "origin_extension" || resp.GetParentGrantId() != parent || resp.GetConsentScope() != "conversation" ||
		resp.GetPolicyMode() != "allowlist" || resp.GetZdr() || resp.GetKeyId() != "" ||
		strings.Join(resp.GetAllowedDomains(), ",") != "api.other.org,docs.other.org" {
		t.Fatalf("response = %#v", resp)
	}
	val, err := srv.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: resp.GetGrantId()})
	if err != nil || val.GetKind() != "origin_extension" || val.GetConsentScope() != "conversation" || val.GetParentGrantId() != parent ||
		val.GetOwnerId() != "user1" || val.GetSessionKey() != key {
		t.Fatalf("validate = %#v, %v", val, err)
	}
	// Revoking the run revokes the extension.
	if _, err := srv.RevokeGrant(context.Background(), &RevokeGrantRequest{GrantId: parent, Reason: "done"}); err != nil {
		t.Fatal(err)
	}
	if _, err := srv.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: resp.GetGrantId()}); status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("validate after the parent was revoked: %v", err)
	}
}

func TestOriginExtension_DenyBeatsApproval(t *testing.T) {
	store := grant.NewMemoryRepository()
	user := userPrincipal(false)
	parent, key := extParent(t, store, user)
	srv := extServer(store, user, true)

	// Everything denied: refused, with a reason and no host in the error.
	_, err := srv.AcquireGrant(context.Background(), extReq(parent, key, func(r *AcquireGrantRequest) {
		r.ConsentDeniedDomains = []string{"other.org"} // above the approved host: a carve-out cannot be expressed
	}))
	if status.Code(err) != codes.PermissionDenied || errorReason(err) != "ORIGIN_EXTENSION_DENIED" {
		t.Fatalf("all denied: code=%v reason=%q", status.Code(err), errorReason(err))
	}
	if strings.Contains(err.Error(), "other.org") {
		t.Fatalf("the refusal names a host: %v", err)
	}

	// Partly denied: the denied host is dropped and reported, the rest granted.
	resp, err := srv.AcquireGrant(context.Background(), extReq(parent, key, func(r *AcquireGrantRequest) {
		r.AllowedDomains = []string{"docs.other.org", "pay.other.net"}
		r.ConsentDeniedDomains = []string{"pay.other.net"}
	}))
	if err != nil {
		t.Fatal(err)
	}
	if strings.Join(resp.GetAllowedDomains(), ",") != "docs.other.org" || strings.Join(resp.GetDeniedDomains(), ",") != "pay.other.net" {
		t.Fatalf("allowed=%v denied=%v", resp.GetAllowedDomains(), resp.GetDeniedDomains())
	}
}

func TestOriginExtension_SitePolicyDenyBeatsTheConsentAndAskIsAnswered(t *testing.T) {
	store := grant.NewMemoryRepository()
	user := userPrincipal(false)
	parent, key := extParent(t, store, user)
	sites := &fakeSites{effective: map[string]string{
		"docs.other.org": sitepolicy.EffectiveAsk, // the person just answered: allowed
		"bank.other.org": sitepolicy.EffectiveDeny,
		"read.other.org": sitepolicy.EffectiveRead, // an extension has no level of its own: dropped
	}}
	srv := extServer(store, user, true).WithOptions(Options{SitePolicy: sitepolicy.ModeEnforce, SiteResolver: sites})
	resp, err := srv.AcquireGrant(context.Background(), extReq(parent, key, func(r *AcquireGrantRequest) {
		r.AllowedDomains = []string{"docs.other.org", "bank.other.org", "read.other.org"}
	}))
	if err != nil {
		t.Fatal(err)
	}
	if strings.Join(resp.GetAllowedDomains(), ",") != "docs.other.org" || len(resp.GetDeniedDomains()) != 2 {
		t.Fatalf("allowed=%v denied=%v", resp.GetAllowedDomains(), resp.GetDeniedDomains())
	}
	// Every host denied: refused; org-core unreadable in enforce: Unavailable.
	_, err = srv.AcquireGrant(context.Background(), extReq(parent, key, func(r *AcquireGrantRequest) {
		r.AllowedDomains = []string{"bank.other.org"}
	}))
	if status.Code(err) != codes.PermissionDenied {
		t.Fatalf("all hosts denied by the site policy: %v", err)
	}
	sites.err = sitepolicy.ErrUnavailable
	_, err = srv.AcquireGrant(context.Background(), extReq(parent, key, func(r *AcquireGrantRequest) {
		r.AllowedDomains = []string{"new.other.org"}
	}))
	if status.Code(err) != codes.Unavailable {
		t.Fatalf("org-core unreadable: %v", err)
	}
}

func TestOriginExtension_OnlyASignedInUserAsksAndOnlyWithThisShape(t *testing.T) {
	store := grant.NewMemoryRepository()
	user := userPrincipal(false)
	parent, key := extParent(t, store, user)
	ctx := context.Background()

	svc := extServer(store, authctx.Principal{
		OrganizationID: "org1", ActorID: "svc-execution", PrincipalType: "service", Scopes: []string{"browser:read", "browser:write"},
	}, true)
	if _, err := svc.AcquireGrant(ctx, extReq(parent, key)); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("service principal: %v", err)
	}

	srv := extServer(store, user, true)
	nine := []string{"a.x.org", "b.x.org", "c.x.org", "d.x.org", "e.x.org", "f.x.org", "g.x.org", "h.x.org", "i.x.org"}
	bad := map[string]func(*AcquireGrantRequest){
		"wildcard":          func(r *AcquireGrantRequest) { r.AllowedDomains = []string{"*.other.org"} },
		"no hosts":          func(r *AcquireGrantRequest) { r.AllowedDomains = nil },
		"nine hosts":        func(r *AcquireGrantRequest) { r.AllowedDomains = nine },
		"internal host":     func(r *AcquireGrantRequest) { r.AllowedDomains = []string{"db.svc"} },
		"no parent":         func(r *AcquireGrantRequest) { r.ParentGrantId = "" },
		"no scope":          func(r *AcquireGrantRequest) { r.ConsentScope = "" },
		"bad scope":         func(r *AcquireGrantRequest) { r.ConsentScope = "forever" },
		"sensitive action":  func(r *AcquireGrantRequest) { r.AllowedActions = []string{"download_ref"} },
		"budget":            func(r *AcquireGrantRequest) { r.BudgetMicroUsd = 1; r.BudgetSource = "request" },
		"read only":         func(r *AcquireGrantRequest) { r.ReadOnly = true },
		"bad denied entry":  func(r *AcquireGrantRequest) { r.ConsentDeniedDomains = []string{"not a host"} },
		"covered by parent": func(r *AcquireGrantRequest) { r.AllowedDomains = []string{"www.example.com"} },
	}
	for name, mutate := range bad {
		if _, err := srv.AcquireGrant(ctx, extReq(parent, key, mutate)); err == nil {
			t.Fatalf("%s: the request was accepted", name)
		}
	}
	// Another user's parent is NotFound; another tenant is refused.
	other := extServer(store, authctx.Principal{OrganizationID: "org1", ActorID: "user2", PrincipalType: "user"}, true)
	if _, err := other.AcquireGrant(ctx, extReq(parent, key)); status.Code(err) != codes.NotFound {
		t.Fatalf("another user's parent: %v", err)
	}
	if _, err := srv.AcquireGrant(ctx, extReq(parent, key, func(r *AcquireGrantRequest) { r.OrgId = "org2" })); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("another tenant: %v", err)
	}
}

func TestOriginExtension_ZdrIsTurnScopeOnly(t *testing.T) {
	store := grant.NewMemoryRepository()
	zdr := userPrincipal(true)
	parent, key := extParent(t, store, zdr)
	srv := extServer(store, zdr, true)
	if _, err := srv.AcquireGrant(context.Background(), extReq(parent, key, func(r *AcquireGrantRequest) { r.ConsentScope = "always" })); status.Code(err) != codes.InvalidArgument {
		t.Fatalf("zdr always: %v", err)
	}
	resp, err := srv.AcquireGrant(context.Background(), extReq(parent, key))
	if err != nil || !resp.GetZdr() || !strings.HasPrefix(resp.GetGrantId(), "z_") {
		t.Fatalf("zdr turn = %#v, %v", resp, err)
	}
}

// A consent field on any other grant would be ignored silently: it is refused.
func TestOriginExtension_ConsentFieldsOnOtherGrantsAreRefused(t *testing.T) {
	store := grant.NewMemoryRepository()
	srv := extServer(store, userPrincipal(false), true)
	for name, mutate := range map[string]func(*AcquireGrantRequest){
		"scope":  func(r *AcquireGrantRequest) { r.ConsentScope = "turn" },
		"denied": func(r *AcquireGrantRequest) { r.ConsentDeniedDomains = []string{"x.org"} },
	} {
		req := &AcquireGrantRequest{OrgId: "org1", SessionKey: "s", AllowedDomains: []string{"example.com"}}
		mutate(req)
		if _, err := srv.AcquireGrant(context.Background(), req); status.Code(err) != codes.InvalidArgument {
			t.Fatalf("%s on an ordinary grant: %v", name, err)
		}
	}
	human := humanReq(func(r *AcquireGrantRequest) { r.ConsentScope = "turn" })
	if _, err := humanServer(store, userPrincipal(false)).AcquireGrant(context.Background(), human); status.Code(err) != codes.InvalidArgument {
		t.Fatalf("scope on human_browse: %v", err)
	}
}

func TestOriginExtension_ErrorMapping(t *testing.T) {
	for err, want := range map[error]codes.Code{
		grant.ErrConsentScopeInvalid:    codes.InvalidArgument,
		grant.ErrConsentScopeZdr:        codes.InvalidArgument,
		grant.ErrWildcardRefused:        codes.InvalidArgument,
		grant.ErrExtensionNothingNew:    codes.InvalidArgument,
		grant.ErrExtensionDenied:        codes.PermissionDenied,
		grant.ErrExtensionHostCap:       codes.ResourceExhausted,
		grant.ErrExtensionParentInvalid: codes.FailedPrecondition,
	} {
		if got := status.Code(mapErr(err)); got != want {
			t.Fatalf("%v maps to %v, want %v", err, got, want)
		}
	}
}
