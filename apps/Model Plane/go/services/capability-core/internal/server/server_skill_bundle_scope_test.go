package server

import (
	"context"
	"testing"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
	"github.com/triodelab/model-plane/services/capability-core/internal/skillbundle"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// scopedBundleStub is a bundle store that also answers the person's visible reads. It records the caller and the admin
// flag each visible read names, and counts its visible reads apart from the plain ones (skillBundleReaderStub.calls).
type scopedBundleStub struct {
	skillBundleReaderStub
	visibleSummaries []skillbundle.Summary
	visibleManifest  skillbundle.Manifest
	visibleErr       error
	visibleCalls     int
	gotCaller        string
	gotAdmin         bool
	gotVisibleSkills []string
}

func (stub *scopedBundleStub) ListVisible(_ context.Context, orgID, callerID string, isAdmin bool, skillIDs []string) ([]skillbundle.Summary, error) {
	stub.visibleCalls++
	stub.gotOrg = orgID
	stub.gotCaller = callerID
	stub.gotAdmin = isAdmin
	stub.gotVisibleSkills = skillIDs
	return stub.visibleSummaries, stub.visibleErr
}

func (stub *scopedBundleStub) ManifestVisible(_ context.Context, orgID, callerID string, isAdmin bool, skillID, manifestSHA256 string) (skillbundle.Manifest, error) {
	stub.visibleCalls++
	stub.gotOrg = orgID
	stub.gotCaller = callerID
	stub.gotAdmin = isAdmin
	stub.gotSkillID = []string{skillID}
	stub.gotDigest = manifestSHA256
	return stub.visibleManifest, stub.visibleErr
}

// personContext is a verified context for a person of org with the given scopes.
func personContext(org, actor string, scopes ...string) context.Context {
	return authctx.ContextWithPrincipal(context.Background(), authctx.Principal{
		OrganizationID: org, ActorID: actor, PrincipalType: "user", Scopes: scopes, RetentionPolicyPresent: true,
	})
}

// serviceContext is a verified context for a service of org with the given scopes.
func serviceContext(org, actor string, scopes ...string) context.Context {
	return authctx.ContextWithPrincipal(context.Background(), authctx.Principal{
		OrganizationID: org, ActorID: actor, PrincipalType: "service", Scopes: scopes, RetentionPolicyPresent: true,
	})
}

// A person's listing under SKILL_BUNDLE_SCOPE_RULES is the visible read, for the caller the verified principal names,
// with the admin flag the verified scopes give. The request names no caller and no admin.
func TestSkillBundleListingFollowsThePersonUnderTheScopeRules(t *testing.T) {
	t.Parallel()

	store := &scopedBundleStub{visibleSummaries: []skillbundle.Summary{{SkillID: "skill-own", ManifestSHA256: "m1"}}}
	on := newTestServer().WithSkillBundles(store, true).WithSkillBundleScopeRules(true)
	list, err := on.ListSkillBundles(personContext("triodelab", "user-a", "org:admin"), &mpv1.ListSkillBundlesRequest{SkillIds: []string{"skill-own"}})
	if err != nil {
		t.Fatalf("ListSkillBundles: %v", err)
	}
	if store.visibleCalls != 1 || store.calls != 0 {
		t.Fatalf("visible reads = %d, plain reads = %d; a person must read only the visible list", store.visibleCalls, store.calls)
	}
	if store.gotCaller != "user-a" || !store.gotAdmin || store.gotOrg != "triodelab" || len(store.gotVisibleSkills) != 1 {
		t.Fatalf("visible read named caller %q admin %v org %q skills %v", store.gotCaller, store.gotAdmin, store.gotOrg, store.gotVisibleSkills)
	}
	if len(list.Bundles) != 1 || list.Bundles[0].SkillId != "skill-own" {
		t.Fatalf("bundles = %+v, want the visible one", list.Bundles)
	}

	if _, err := on.ListSkillBundles(personContext("triodelab", "user-b"), &mpv1.ListSkillBundlesRequest{}); err != nil {
		t.Fatalf("ListSkillBundles for a member: %v", err)
	}
	if store.gotCaller != "user-b" || store.gotAdmin {
		t.Fatalf("a member was read as caller %q admin %v", store.gotCaller, store.gotAdmin)
	}
}

// A service holding the capability read scope keeps the unfiltered listing and manifest, and a person is never served a
// bundle body. The body read stays the service's.
func TestSkillBundleReadsStayUnfilteredForAServiceAndBodiesStayServiceOnly(t *testing.T) {
	t.Parallel()

	store := &scopedBundleStub{
		skillBundleReaderStub: skillBundleReaderStub{
			summaries: []skillbundle.Summary{{SkillID: "skill-a"}, {SkillID: "skill-b"}},
			manifest:  skillbundle.Manifest{SkillID: "skill-a", ManifestSHA256: "m1", Files: []skillbundle.File{{Path: "SKILL.md", SHA256: "d1", Size: 6}}},
			blob:      []byte("body"),
		},
	}
	on := newTestServer().WithSkillBundles(store, true).WithSkillBundleScopeRules(true)
	service := serviceContext("triodelab", "execution-core", authz.ReadScope)

	list, err := on.ListSkillBundles(service, &mpv1.ListSkillBundlesRequest{})
	if err != nil || len(list.Bundles) != 2 || store.visibleCalls != 0 {
		t.Fatalf("service listing = %+v, %v with %d visible reads; want both bundles unfiltered", list, err, store.visibleCalls)
	}
	if _, err := on.GetSkillBundleManifest(service, &mpv1.GetSkillBundleManifestRequest{SkillId: "skill-a"}); err != nil {
		t.Fatalf("service manifest: %v", err)
	}
	if store.visibleCalls != 0 {
		t.Fatalf("a service's manifest read reached the visible read")
	}
	if blob, err := on.GetSkillBlob(service, &mpv1.GetSkillBlobRequest{SkillId: "skill-a", Sha256: "d1"}); err != nil || string(blob.Content) != "body" {
		t.Fatalf("service blob = %v, %v; want the body", blob, err)
	}

	person := personContext("triodelab", "user-a")
	if _, err := on.GetSkillBlob(person, &mpv1.GetSkillBlobRequest{SkillId: "skill-a", Sha256: "d1"}); status.Code(err) != codes.NotFound {
		t.Fatalf("person blob status = %v, want NotFound: a body is a service's read", status.Code(err))
	}
}

// A skill the person may not see answers NotFound with the same message a missing bundle answers, never PermissionDenied.
func TestSkillBundleManifestAnswersAnInvisibleSkillAsMissing(t *testing.T) {
	t.Parallel()

	store := &scopedBundleStub{visibleErr: skillbundle.ErrNotFound}
	on := newTestServer().WithSkillBundles(store, true).WithSkillBundleScopeRules(true)
	_, invisible := on.GetSkillBundleManifest(personContext("triodelab", "user-a"), &mpv1.GetSkillBundleManifestRequest{SkillId: "skill-secret"})
	if status.Code(invisible) != codes.NotFound || status.Convert(invisible).Message() != "skill bundle not found" {
		t.Fatalf("invisible manifest = %v, want the missing-bundle NotFound", invisible)
	}
	if store.gotSkillID[0] != "skill-secret" || store.gotCaller != "user-a" {
		t.Fatalf("visible manifest read named skill %v caller %q", store.gotSkillID, store.gotCaller)
	}
}

// With the digest of a kept bundle named, a person reads it through the visible read only with retention on, as the
// plain read honours the digest only with retention on.
func TestSkillBundleManifestHonoursTheDigestOnlyWithRetention(t *testing.T) {
	t.Parallel()

	digest := "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
	store := &scopedBundleStub{}
	off := newTestServer().WithSkillBundles(store, true).WithSkillBundleScopeRules(true)
	if _, err := off.GetSkillBundleManifest(personContext("triodelab", "user-a"), &mpv1.GetSkillBundleManifestRequest{SkillId: "skill-a", ManifestSha256: digest}); err != nil {
		t.Fatalf("manifest without retention: %v", err)
	}
	if store.gotDigest != "" {
		t.Fatalf("digest %q reached the visible read without retention", store.gotDigest)
	}

	on := newTestServer().WithSkillBundles(store, true).WithSkillBundleScopeRules(true).WithSkillBundleRetention(true)
	if _, err := on.GetSkillBundleManifest(personContext("triodelab", "user-a"), &mpv1.GetSkillBundleManifestRequest{SkillId: "skill-a", ManifestSha256: digest}); err != nil {
		t.Fatalf("manifest with retention: %v", err)
	}
	if store.gotDigest != digest {
		t.Fatalf("digest reached the visible read as %q, want %q", store.gotDigest, digest)
	}
	if _, err := on.GetSkillBundleManifest(personContext("triodelab", "user-a"), &mpv1.GetSkillBundleManifestRequest{SkillId: "skill-a", ManifestSha256: "not-a-digest"}); status.Code(err) != codes.NotFound {
		t.Fatalf("a malformed digest = %v, want NotFound before any read", status.Code(err))
	}
}

// With SKILL_BUNDLE_SCOPE_RULES off, a person's reads are the unfiltered ones they were, and the visible read is never
// consulted.
func TestSkillBundleScopeRulesOffKeepTheUnfilteredReads(t *testing.T) {
	t.Parallel()

	store := &scopedBundleStub{skillBundleReaderStub: skillBundleReaderStub{summaries: []skillbundle.Summary{{SkillID: "skill-a"}, {SkillID: "skill-b"}}}}
	off := newTestServer().WithSkillBundles(store, true)
	list, err := off.ListSkillBundles(personContext("triodelab", "user-a"), &mpv1.ListSkillBundlesRequest{})
	if err != nil || len(list.Bundles) != 2 {
		t.Fatalf("person listing with the scope rules off = %+v, %v; want both bundles", list, err)
	}
	if store.visibleCalls != 0 || store.calls != 1 {
		t.Fatalf("visible reads = %d, plain reads = %d; want the plain read only", store.visibleCalls, store.calls)
	}
}

// A store without the visible read answers a person nothing rather than the unfiltered bundles, so a missing filter can
// never widen what a person sees.
func TestSkillBundleScopeRulesWithoutAVisibleReaderServeAPersonNothing(t *testing.T) {
	t.Parallel()

	store := &skillBundleReaderStub{
		summaries: []skillbundle.Summary{{SkillID: "skill-a"}},
		manifest:  skillbundle.Manifest{SkillID: "skill-a", ManifestSHA256: "m1"},
	}
	on := newTestServer().WithSkillBundles(store, true).WithSkillBundleScopeRules(true)
	list, err := on.ListSkillBundles(personContext("triodelab", "user-a"), &mpv1.ListSkillBundlesRequest{})
	if err != nil || len(list.Bundles) != 0 {
		t.Fatalf("listing = %+v, %v; want nothing for a person", list, err)
	}
	if _, err := on.GetSkillBundleManifest(personContext("triodelab", "user-a"), &mpv1.GetSkillBundleManifestRequest{SkillId: "skill-a"}); status.Code(err) != codes.NotFound {
		t.Fatalf("manifest = %v, want NotFound for a person", status.Code(err))
	}
	if store.calls != 0 {
		t.Fatalf("a person's read reached the unfiltered store %d times", store.calls)
	}
}

// Only a service keeps the unfiltered read under SKILL_BUNDLE_SCOPE_RULES. Any other principal type, here an API key
// acting for its owning user, is bounded by the visible read, so an unknown type fails closed.
func TestSkillBundleScopeRulesBoundEveryPrincipalButAService(t *testing.T) {
	t.Parallel()

	store := &scopedBundleStub{visibleSummaries: []skillbundle.Summary{{SkillID: "skill-own"}}}
	on := newTestServer().WithSkillBundles(store, true).WithSkillBundleScopeRules(true)
	keyed := authctx.ContextWithPrincipal(context.Background(), authctx.Principal{
		OrganizationID: "triodelab", ActorID: "user-a", PrincipalType: "api_key", RetentionPolicyPresent: true,
	})
	list, err := on.ListSkillBundles(keyed, &mpv1.ListSkillBundlesRequest{})
	if err != nil || len(list.Bundles) != 1 || store.visibleCalls != 1 || store.calls != 0 {
		t.Fatalf("an API key's listing = %+v, %v with %d visible and %d plain reads; want the visible read only", list, err, store.visibleCalls, store.calls)
	}
	if store.gotCaller != "user-a" {
		t.Fatalf("the API key's listing named caller %q, want its owning user", store.gotCaller)
	}
}
