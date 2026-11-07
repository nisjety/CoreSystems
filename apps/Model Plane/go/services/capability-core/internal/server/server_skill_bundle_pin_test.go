package server

import (
	"context"
	"strings"
	"testing"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/capability-core/internal/skillbundle"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// pinnedBundleStub serves the live bundle through Manifest and a kept one through ManifestAt.
type pinnedBundleStub struct {
	skillBundleReaderStub
	pinned    skillbundle.Manifest
	atCalls   int
	gotPinned string
}

func (stub *pinnedBundleStub) ManifestAt(_ context.Context, orgID, skillID, manifestSHA256 string) (skillbundle.Manifest, error) {
	stub.atCalls++
	stub.gotOrg = orgID
	stub.gotSkillID = []string{skillID}
	stub.gotPinned = manifestSHA256
	return stub.pinned, nil
}

// EP-11 S5: with retention on, a manifest digest in the request reads that bundle, so a run pinned
// to an older version reads its own files; off, or with no digest, the live bundle is read as before.
func TestGetSkillBundleManifestReadsThePinnedBundleOnlyWithRetention(t *testing.T) {
	t.Parallel()

	pinned := strings.Repeat("a", 64)
	store := &pinnedBundleStub{
		skillBundleReaderStub: skillBundleReaderStub{manifest: skillbundle.Manifest{SkillID: "skill-a", ManifestSHA256: "live",
			Files: []skillbundle.File{{Path: "SKILL.md", SHA256: "d1", Size: 1}}}},
		pinned: skillbundle.Manifest{SkillID: "skill-a", ManifestSHA256: pinned,
			Files: []skillbundle.File{{Path: "SKILL.md", SHA256: "d0", Size: 1}}},
	}
	ctx := verifiedGRPCContext(t, "triodelab")
	request := &mpv1.GetSkillBundleManifestRequest{SkillId: "skill-a", ManifestSha256: pinned}

	off := newTestServer().WithSkillBundles(store, true)
	manifest, err := off.GetSkillBundleManifest(ctx, request)
	if err != nil || manifest.ManifestSha256 != "live" || store.atCalls != 0 {
		t.Fatalf("retention off: manifest = %+v, err = %v, pinned reads = %d; want the live bundle", manifest, err, store.atCalls)
	}

	on := newTestServer().WithSkillBundles(store, true).WithSkillBundleRetention(true)
	manifest, err = on.GetSkillBundleManifest(ctx, request)
	if err != nil || manifest.ManifestSha256 != pinned || store.gotPinned != pinned || store.gotOrg != "triodelab" {
		t.Fatalf("retention on: manifest = %+v, err = %v, read %q for %q", manifest, err, store.gotPinned, store.gotOrg)
	}
	if manifest, err = on.GetSkillBundleManifest(ctx, &mpv1.GetSkillBundleManifestRequest{SkillId: "skill-a"}); err != nil || manifest.ManifestSha256 != "live" {
		t.Fatalf("no digest: manifest = %+v, err = %v; want the live bundle", manifest, err)
	}

	before := store.atCalls
	if _, err := on.GetSkillBundleManifest(ctx, &mpv1.GetSkillBundleManifestRequest{SkillId: "skill-a", ManifestSha256: "../x"}); status.Code(err) != codes.NotFound {
		t.Fatalf("malformed digest status = %v, want NotFound", status.Code(err))
	}
	if store.atCalls != before {
		t.Fatal("a malformed digest must not reach the store")
	}
}
