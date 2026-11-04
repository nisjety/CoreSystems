package skillbundle

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// EP-11 S2: with SKILL_VERSIONS_API off the store writes only its own tables. A nil transaction
// proves the stamp touches nothing: any statement on it would panic.
func TestSkillStampIsInertWhileTheSwitchIsOff(t *testing.T) {
	store := NewStore(nil)
	if err := store.stampSkill(context.Background(), nil, "org-a", "skill-1", "user-a", "abc"); err != nil {
		t.Fatalf("stamp with the switch off = %v, want nil", err)
	}
	if !NewStore(nil).WithSkillVersions(true).skillVersions {
		t.Fatal("WithSkillVersions(true) must turn the stamp on")
	}
}

// The stamp names the uploader with the settings session-core's version trigger reads, and writes
// only the bundle digest column, so the trigger stays the single writer of version and digest.
func TestSkillStampNamesTheActorTheVersionTriggerReads(t *testing.T) {
	migration, err := os.ReadFile(filepath.Join("..", "..", "..", "..", "..", "rust", "services", "session-core", "migrations", "0096_agent_skill_versions.sql"))
	if err != nil {
		t.Skipf("the session-core migration is not beside this module: %v", err)
	}
	for _, setting := range []string{"mp.actor_kind", "mp.actor_id"} {
		if !strings.Contains(stampSkillActorSQL, "set_config('"+setting+"'") {
			t.Errorf("stampSkillActorSQL does not set %s", setting)
		}
		if !strings.Contains(string(migration), "current_setting('"+setting+"', true)") {
			t.Errorf("the version trigger does not read %s", setting)
		}
	}
	if !strings.Contains(string(migration), "'bundle_sha256', s.bundle_sha256") {
		t.Error("the version body must cover bundle_sha256, or a bundle change cuts no version")
	}
	if strings.Contains(stampSkillBundleSQL, "version") || strings.Contains(stampSkillBundleSQL, "digest =") {
		t.Errorf("the stamp must not write version or digest: %s", stampSkillBundleSQL)
	}
}
