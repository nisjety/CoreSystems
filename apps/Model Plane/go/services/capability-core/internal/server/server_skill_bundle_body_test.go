package server

import (
	"context"
	"testing"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// A bundle body is what the sandbox runs, so under SKILL_BUNDLE_SCOPE_RULES no person is served one while the bundle
// surface is on, whichever of the person's principal types asks (F-f03-08, decision D8, register D2-383). The body
// reads the store only for a service.
func TestNoPersonIsServedABundleBodyUnderTheScopeRules(t *testing.T) {
	t.Parallel()

	persons := []struct {
		name      string
		principal authctx.Principal
	}{
		{name: "a member", principal: authctx.Principal{OrganizationID: "triodelab", ActorID: "user-a", PrincipalType: "user", RetentionPolicyPresent: true}},
		{name: "a member holding the read and write scopes", principal: authctx.Principal{OrganizationID: "triodelab", ActorID: "user-a", PrincipalType: "user", Scopes: []string{authz.ReadScope, authz.WriteScope}, RetentionPolicyPresent: true}},
		{name: "an API key acting for its owner", principal: authctx.Principal{OrganizationID: "triodelab", ActorID: "user-a", PrincipalType: "api_key", RetentionPolicyPresent: true}},
	}
	store := &skillBundleReaderStub{blob: []byte("#!/bin/sh\necho run\n")}
	on := newTestServer().WithSkillBundles(store, true).WithSkillBundleScopeRules(true)
	for _, person := range persons {
		ctx := authctx.ContextWithPrincipal(context.Background(), person.principal)
		if _, err := on.GetSkillBlob(ctx, &mpv1.GetSkillBlobRequest{SkillId: "skill-a", Sha256: "d1"}); status.Code(err) != codes.NotFound {
			t.Fatalf("%s got %v under the scope rules, want NotFound", person.name, status.Code(err))
		}
	}
	if store.calls != 0 {
		t.Fatalf("a person's body read reached the store %d times under the scope rules", store.calls)
	}
}

// With SKILL_BUNDLE_SCOPE_RULES off, a person's body read is as it was: the server serves it, and the read rule in
// AuthorizeGRPCWith (SKILL_BUNDLE_READ_RULE_ENABLED) is the gate, so this switch changes nothing.
func TestAPersonsBodyReadIsUnchangedWithTheScopeRulesOff(t *testing.T) {
	t.Parallel()

	store := &skillBundleReaderStub{blob: []byte("body")}
	off := newTestServer().WithSkillBundles(store, true).WithSkillBundleScopeRules(false)
	member := authctx.ContextWithPrincipal(context.Background(), authctx.Principal{
		OrganizationID: "triodelab", ActorID: "user-a", PrincipalType: "user", RetentionPolicyPresent: true,
	})
	blob, err := off.GetSkillBlob(member, &mpv1.GetSkillBlobRequest{SkillId: "skill-a", Sha256: "d1"})
	if err != nil || string(blob.Content) != "body" || store.calls != 1 {
		t.Fatalf("scope rules off: a member's blob = %v, %v with %d store reads; want the body from one read", blob, err, store.calls)
	}
}

// The service that holds the capability read scope is still served the body, under either setting of the scope rules.
func TestAServiceWithTheReadScopeIsStillServedABundleBody(t *testing.T) {
	t.Parallel()

	service := authctx.ContextWithPrincipal(context.Background(), authctx.Principal{
		OrganizationID: "triodelab", ActorID: "execution-core", PrincipalType: "service", Scopes: []string{authz.ReadScope}, RetentionPolicyPresent: true,
	})
	for _, scopeRules := range []bool{false, true} {
		store := &skillBundleReaderStub{blob: []byte("body")}
		on := newTestServer().WithSkillBundles(store, true).WithSkillBundleScopeRules(scopeRules)
		blob, err := on.GetSkillBlob(service, &mpv1.GetSkillBlobRequest{SkillId: "skill-a", Sha256: "d1"})
		if err != nil || string(blob.Content) != "body" || store.calls != 1 {
			t.Fatalf("scope rules %v: service blob = %v, %v with %d store reads; want the body from one read", scopeRules, blob, err, store.calls)
		}
	}
}

// With SKILL_BUNDLES_ENABLED off, every caller is answered NotFound and the store is not read, as before.
func TestBundleBodiesStayNotFoundWhileTheBundleSurfaceIsOff(t *testing.T) {
	t.Parallel()

	store := &skillBundleReaderStub{blob: []byte("body")}
	off := newTestServer().WithSkillBundles(store, false)
	callers := []authctx.Principal{
		{OrganizationID: "triodelab", ActorID: "user-a", PrincipalType: "user", RetentionPolicyPresent: true},
		{OrganizationID: "triodelab", ActorID: "execution-core", PrincipalType: "service", Scopes: []string{authz.ReadScope}, RetentionPolicyPresent: true},
	}
	for _, caller := range callers {
		ctx := authctx.ContextWithPrincipal(context.Background(), caller)
		if _, err := off.GetSkillBlob(ctx, &mpv1.GetSkillBlobRequest{SkillId: "skill-a", Sha256: "d1"}); status.Code(err) != codes.NotFound {
			t.Fatalf("%s with the bundle surface off got %v, want NotFound", caller.PrincipalType, status.Code(err))
		}
	}
	if store.calls != 0 {
		t.Fatalf("bodies reached the store %d times with the bundle surface off", store.calls)
	}
}
