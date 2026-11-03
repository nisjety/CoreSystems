package orgkeys

import (
	"bytes"
	"context"
	"encoding/base64"
	"errors"
	"fmt"
	"strings"
	"testing"
)

func testKeyB64(fill byte) string {
	return base64.StdEncoding.EncodeToString(bytes.Repeat([]byte{fill}, KeySize))
}

func envFrom(values map[string]string) func(string) string {
	return func(name string) string { return values[name] }
}

func testMaster(t *testing.T, active string, keys map[string]byte) *EnvMasterKeys {
	t.Helper()
	parts := make([]string, 0, len(keys))
	for id, fill := range keys {
		parts = append(parts, fmt.Sprintf("%q:%q", id, testKeyB64(fill)))
	}
	master, err := LoadEnvMasterKeys(envFrom(map[string]string{
		EnvMasterKeys:   "{" + strings.Join(parts, ",") + "}",
		EnvMasterActive: active,
	}))
	if err != nil {
		t.Fatalf("load master keys: %v", err)
	}
	return master
}

type recordingPublisher struct {
	subjects []string
	payloads []map[string]any
}

func (r *recordingPublisher) PublishAudit(_ context.Context, subject, _ string, payload map[string]any) error {
	r.subjects = append(r.subjects, subject)
	r.payloads = append(r.payloads, payload)
	return nil
}

func newTestService(t *testing.T, orgs ...string) (*Service, *MemoryStore, *recordingPublisher) {
	t.Helper()
	store := NewMemoryStore(orgs...)
	service := NewService(store, testMaster(t, "v1", map[string]byte{"v1": 0x11}))
	publisher := &recordingPublisher{}
	service.SetAuditPublisher(publisher)
	return service, store, publisher
}

var profileA = map[string]string{"profile_id": "prof_a"}

func TestGenerateUnwrapRoundtrip(t *testing.T) {
	service, store, publisher := newTestService(t, "org-a")
	ctx := context.Background()
	key, err := service.GenerateDataKey(ctx, "org-a", "quarry-edge", PurposeQuarryProfile, profileA)
	if err != nil {
		t.Fatalf("generate: %v", err)
	}
	if key.KeyID != "oc:1" || key.KEKVersion != 1 || len(key.DEK) != KeySize || len(key.WrappedDEK) != WrappedDEKSize {
		t.Fatalf("unexpected data key shape: id=%s version=%d dek=%d wrapped=%d", key.KeyID, key.KEKVersion, len(key.DEK), len(key.WrappedDEK))
	}
	if bytes.Contains(key.WrappedDEK, key.DEK) {
		t.Fatal("wrapped DEK contains the plaintext DEK")
	}
	dek, current, err := service.Unwrap(ctx, "org-a", "quarry-edge", PurposeQuarryProfile, profileA, key.KeyID, key.WrappedDEK)
	if err != nil {
		t.Fatalf("unwrap: %v", err)
	}
	if !bytes.Equal(dek, key.DEK) || current != 1 {
		t.Fatalf("unwrap returned another key or version %d", current)
	}
	if service.UnwrapCount() != 1 {
		t.Fatalf("unwrap counter = %d, want 1", service.UnwrapCount())
	}
	// A second data key reuses the KEK: one creation event only.
	if _, err := service.GenerateDataKey(ctx, "org-a", "quarry-edge", PurposeQuarryProfile, profileA); err != nil {
		t.Fatalf("second generate: %v", err)
	}
	if events := store.AuditEvents(); len(events) != 1 || events[0] != AuditKEKCreated {
		t.Fatalf("audit events = %v, want one %s", events, AuditKEKCreated)
	}
	if len(publisher.subjects) != 1 || publisher.subjects[0] != AuditSubject {
		t.Fatalf("published = %v", publisher.subjects)
	}
}

func TestUnwrapWithWrongOrgOrProfileContextFails(t *testing.T) {
	service, store, _ := newTestService(t, "org-a", "org-b")
	ctx := context.Background()
	key, err := service.GenerateDataKey(ctx, "org-a", "quarry-edge", PurposeQuarryProfile, profileA)
	if err != nil {
		t.Fatalf("generate: %v", err)
	}
	other := map[string]string{"profile_id": "prof_b"}
	if _, _, err := service.Unwrap(ctx, "org-a", "quarry-edge", PurposeQuarryProfile, other, key.KeyID, key.WrappedDEK); !errors.Is(err, ErrInvalidContext) {
		t.Fatalf("wrong profile: err=%v, want ErrInvalidContext", err)
	}
	// org-b has no KEK v1 yet: the version is not found.
	if _, _, err := service.Unwrap(ctx, "org-b", "quarry-edge", PurposeQuarryProfile, profileA, key.KeyID, key.WrappedDEK); !errors.Is(err, ErrNoKey) {
		t.Fatalf("foreign org without kek: err=%v, want ErrNoKey", err)
	}
	// With its own v1, org-b still cannot open org-a's blob.
	if _, err := service.GenerateDataKey(ctx, "org-b", "quarry-edge", PurposeQuarryProfile, profileA); err != nil {
		t.Fatalf("generate org-b: %v", err)
	}
	if _, _, err := service.Unwrap(ctx, "org-b", "quarry-edge", PurposeQuarryProfile, profileA, key.KeyID, key.WrappedDEK); !errors.Is(err, ErrInvalidContext) {
		t.Fatalf("foreign org: err=%v, want ErrInvalidContext", err)
	}
	tampered := append([]byte(nil), key.WrappedDEK...)
	tampered[len(tampered)-1] ^= 0x01
	if _, _, err := service.Unwrap(ctx, "org-a", "quarry-edge", PurposeQuarryProfile, profileA, key.KeyID, tampered); !errors.Is(err, ErrInvalidContext) {
		t.Fatalf("tampered: err=%v, want ErrInvalidContext", err)
	}
	for name, bad := range map[string]map[string]string{
		"no profile":  {},
		"extra key":   {"profile_id": "prof_a", "user_id": "u"},
		"pipe in id":  {"profile_id": "prof|a"},
		"empty id":    {"profile_id": ""},
		"other field": {"snapshot_id": "s"},
	} {
		if _, err := service.GenerateDataKey(ctx, "org-a", "quarry-edge", PurposeQuarryProfile, bad); !errors.Is(err, ErrInvalidContext) {
			t.Fatalf("%s: err=%v, want ErrInvalidContext", name, err)
		}
	}
	if _, err := service.GenerateDataKey(ctx, "org-a", "quarry-edge", "other.purpose", profileA); !errors.Is(err, ErrInvalidContext) {
		t.Fatalf("unknown purpose: err=%v", err)
	}
	denied := 0
	for _, payload := range store.AuditPayloads() {
		if payload["event"] == AuditUnwrapDenied {
			denied++
			if payload["outcome"] != "denied" {
				t.Fatalf("denied unwrap outcome = %v", payload["outcome"])
			}
		}
	}
	if denied != 4 {
		t.Fatalf("denied unwrap audit events = %d, want 4", denied)
	}
}

func TestUnknownOrgIsNotFound(t *testing.T) {
	service, _, _ := newTestService(t, "org-a")
	if _, err := service.GenerateDataKey(context.Background(), "org-missing", "quarry-edge", PurposeQuarryProfile, profileA); !errors.Is(err, ErrOrgNotFound) {
		t.Fatalf("err=%v, want ErrOrgNotFound", err)
	}
}

func TestDestroyMakesUnwrapGone(t *testing.T) {
	service, store, _ := newTestService(t, "org-a", "org-empty")
	ctx := context.Background()
	key, err := service.GenerateDataKey(ctx, "org-a", "quarry-edge", PurposeQuarryProfile, profileA)
	if err != nil {
		t.Fatalf("generate: %v", err)
	}
	if _, err := service.Rotate(ctx, "org-a", "operator"); err != nil {
		t.Fatalf("rotate: %v", err)
	}
	count, err := service.Destroy(ctx, "org-a", "operator")
	if err != nil || count != 2 {
		t.Fatalf("destroy: count=%d err=%v, want 2 versions", count, err)
	}
	if _, _, err := service.Unwrap(ctx, "org-a", "quarry-edge", PurposeQuarryProfile, profileA, key.KeyID, key.WrappedDEK); !errors.Is(err, ErrDestroyed) {
		t.Fatalf("unwrap after destroy: err=%v, want ErrDestroyed", err)
	}
	if _, err := service.GenerateDataKey(ctx, "org-a", "quarry-edge", PurposeQuarryProfile, profileA); !errors.Is(err, ErrDestroyed) {
		t.Fatalf("generate after destroy: err=%v, want ErrDestroyed (no fresh KEK after shred)", err)
	}
	if _, err := service.Destroy(ctx, "org-a", "operator"); !errors.Is(err, ErrDestroyed) {
		t.Fatalf("second destroy: err=%v, want ErrDestroyed", err)
	}
	for _, rec := range store.keks["org-a"] {
		if rec.Wrapped != nil || rec.State != StateDestroyed {
			t.Fatalf("version %d kept key material or state %s", rec.Version, rec.State)
		}
	}
	meta, err := service.Metadata(ctx, "org-a")
	if err != nil || meta.State != StateDestroyed || meta.ActiveVersion != 0 || len(meta.Versions) != 2 {
		t.Fatalf("metadata after destroy: %+v err=%v", meta, err)
	}
	// An org that never had a KEK is shredded too: the tombstone keeps it so.
	if count, err := service.Destroy(ctx, "org-empty", "operator"); err != nil || count != 0 {
		t.Fatalf("destroy empty org: count=%d err=%v", count, err)
	}
	if _, err := service.GenerateDataKey(ctx, "org-empty", "quarry-edge", PurposeQuarryProfile, profileA); !errors.Is(err, ErrDestroyed) {
		t.Fatalf("generate after empty destroy: err=%v, want ErrDestroyed", err)
	}
}

func TestHardDeleteCascadeRemovesKeys(t *testing.T) {
	service, store, _ := newTestService(t, "org-a")
	ctx := context.Background()
	key, err := service.GenerateDataKey(ctx, "org-a", "quarry-edge", PurposeQuarryProfile, profileA)
	if err != nil {
		t.Fatalf("generate: %v", err)
	}
	store.DeleteOrg("org-a")
	if _, _, err := service.Unwrap(ctx, "org-a", "quarry-edge", PurposeQuarryProfile, profileA, key.KeyID, key.WrappedDEK); !errors.Is(err, ErrOrgNotFound) {
		t.Fatalf("unwrap after org delete: err=%v, want ErrOrgNotFound", err)
	}
}

func TestRotateThenRewrapKeepsDEK(t *testing.T) {
	service, _, _ := newTestService(t, "org-a")
	ctx := context.Background()
	key, err := service.GenerateDataKey(ctx, "org-a", "quarry-edge", PurposeQuarryProfile, profileA)
	if err != nil {
		t.Fatalf("generate: %v", err)
	}
	rotated, err := service.Rotate(ctx, "org-a", "operator")
	if err != nil || rotated.Version != 2 || rotated.State != StateActive {
		t.Fatalf("rotate: %+v err=%v", rotated, err)
	}
	dek, current, err := service.Unwrap(ctx, "org-a", "quarry-edge", PurposeQuarryProfile, profileA, key.KeyID, key.WrappedDEK)
	if err != nil || current != 2 || !bytes.Equal(dek, key.DEK) {
		t.Fatalf("unwrap under retired kek: current=%d err=%v", current, err)
	}
	rewrapped, err := service.Rewrap(ctx, "org-a", "quarry-edge", PurposeQuarryProfile, profileA, key.KeyID, key.WrappedDEK)
	if err != nil || rewrapped.KeyID != "oc:2" || rewrapped.DEK != nil {
		t.Fatalf("rewrap: %+v err=%v", rewrapped, err)
	}
	again, _, err := service.Unwrap(ctx, "org-a", "quarry-edge", PurposeQuarryProfile, profileA, rewrapped.KeyID, rewrapped.WrappedDEK)
	if err != nil || !bytes.Equal(again, key.DEK) {
		t.Fatalf("unwrap rewrapped: err=%v equal=%v", err, bytes.Equal(again, key.DEK))
	}
	meta, err := service.Metadata(ctx, "org-a")
	if err != nil || meta.ActiveVersion != 2 || len(meta.Versions) != 2 || meta.Versions[0].State != StateRetired {
		t.Fatalf("metadata: %+v err=%v", meta, err)
	}
}

func TestCanaryFailureIsUnavailable(t *testing.T) {
	store := NewMemoryStore("org-a")
	// A canary written under another key for the same id: the configured key is wrong.
	wrong := bytes.Repeat([]byte{0x22}, KeySize)
	sealed, err := seal(wrong, []byte(canaryPlaintext), canaryAAD("v1"))
	if err != nil {
		t.Fatalf("seal: %v", err)
	}
	store.SetCanary("v1", sealed)
	service := NewService(store, testMaster(t, "v1", map[string]byte{"v1": 0x11}))
	_, err = service.GenerateDataKey(context.Background(), "org-a", "quarry-edge", PurposeQuarryProfile, profileA)
	if !errors.Is(err, ErrUnavailable) {
		t.Fatalf("err=%v, want ErrUnavailable", err)
	}
	if strings.Contains(strings.ToLower(err.Error()), "decrypt") {
		t.Fatalf("canary failure must not read as a decrypt error: %v", err)
	}
	if len(store.keks["org-a"]) != 0 {
		t.Fatal("a KEK was created under an unproven master key")
	}

	// A stored canary whose master id is no longer configured is a missing key.
	missing := NewMemoryStore("org-a")
	missing.SetCanary("v0", sealed)
	service = NewService(missing, testMaster(t, "v1", map[string]byte{"v1": 0x11}))
	if err := service.Ready(context.Background()); !errors.Is(err, ErrUnavailable) {
		t.Fatalf("missing master key: err=%v, want ErrUnavailable", err)
	}

	// The right key writes a canary on first use and then serves.
	fresh := NewMemoryStore("org-a")
	service = NewService(fresh, testMaster(t, "v1", map[string]byte{"v1": 0x11}))
	if err := service.Ready(context.Background()); err != nil {
		t.Fatalf("ready: %v", err)
	}
	if _, ok := fresh.canaries["v1"]; !ok {
		t.Fatal("no canary stored for v1")
	}
}

func TestLoadEnvMasterKeysRejectsBadConfigWithoutEchoingKeys(t *testing.T) {
	good := testKeyB64(0x33)
	cases := map[string]map[string]string{
		"unset":         {},
		"not json":      {EnvMasterKeys: "v1=" + good, EnvMasterActive: "v1"},
		"short key":     {EnvMasterKeys: `{"v1":"` + base64.StdEncoding.EncodeToString([]byte("short-secret-value")) + `"}`, EnvMasterActive: "v1"},
		"zero key":      {EnvMasterKeys: `{"v1":"` + base64.StdEncoding.EncodeToString(make([]byte, KeySize)) + `"}`, EnvMasterActive: "v1"},
		"no active":     {EnvMasterKeys: `{"v1":"` + good + `"}`},
		"active absent": {EnvMasterKeys: `{"v1":"` + good + `"}`, EnvMasterActive: "v2"},
		"bad id":        {EnvMasterKeys: `{"V 1":"` + good + `"}`, EnvMasterActive: "V 1"},
		"duplicate":     {EnvMasterKeys: `{"v1":"` + good + `","v2":"` + good + `"}`, EnvMasterActive: "v1"},
	}
	for name, env := range cases {
		_, err := LoadEnvMasterKeys(envFrom(env))
		if err == nil {
			t.Fatalf("%s: accepted", name)
		}
		if strings.Contains(err.Error(), good) || strings.Contains(err.Error(), "short-secret-value") {
			t.Fatalf("%s: error echoes key material: %v", name, err)
		}
	}
	if err := ValidateConfig(envFrom(map[string]string{})); err != nil {
		t.Fatalf("disabled config must pass: %v", err)
	}
	if err := ValidateConfig(envFrom(map[string]string{EnvEnabled: "1"})); err == nil {
		t.Fatal("enabled without master keys must fail the boot assertion")
	}
	if err := ValidateConfig(envFrom(map[string]string{EnvEnabled: "1", EnvMasterKeys: `{"v1":"` + good + `"}`, EnvMasterActive: "v1"})); err != nil {
		t.Fatalf("enabled with master keys: %v", err)
	}
}

func TestAuditPayloadsCarryNoKeyMaterial(t *testing.T) {
	service, store, _ := newTestService(t, "org-a")
	ctx := context.Background()
	key, err := service.GenerateDataKey(ctx, "org-a", "quarry-edge", PurposeQuarryProfile, profileA)
	if err != nil {
		t.Fatalf("generate: %v", err)
	}
	_, _, _ = service.Unwrap(ctx, "org-a", "quarry-edge", PurposeQuarryProfile, map[string]string{"profile_id": "prof_b"}, key.KeyID, key.WrappedDEK)
	_, _ = service.Rotate(ctx, "org-a", "operator")
	_, _ = service.Destroy(ctx, "org-a", "operator")
	secrets := []string{
		base64.StdEncoding.EncodeToString(key.DEK),
		base64.StdEncoding.EncodeToString(key.WrappedDEK),
		testKeyB64(0x11),
		"prof_a", "prof_b",
	}
	for _, payload := range store.AuditPayloads() {
		rendered := fmt.Sprintf("%v", payload)
		for _, secret := range secrets {
			if strings.Contains(rendered, secret) {
				t.Fatalf("audit payload carries %q: %s", secret, rendered)
			}
		}
	}
	if events := store.AuditEvents(); strings.Join(events, ",") != strings.Join([]string{AuditKEKCreated, AuditUnwrapDenied, AuditKEKRotated, AuditKeysDestroy}, ",") {
		t.Fatalf("audit events = %v", events)
	}
}

func TestParseKeyID(t *testing.T) {
	for _, good := range []string{"oc:1", "oc:42", "oc:1000000"} {
		if _, err := ParseKeyID(good); err != nil {
			t.Fatalf("%s rejected: %v", good, err)
		}
	}
	for _, bad := range []string{"", "oc:", "oc:0", "oc:01", "oc:-1", "env:1", "oc:1000001", "oc:1a", "OC:1"} {
		if _, err := ParseKeyID(bad); err == nil {
			t.Fatalf("%s accepted", bad)
		}
	}
}
