package api

import (
	"context"
	"strings"
	"testing"
)

// A plugin removal deletes the skills the plugin created as the skill delete route does, so the skill's bundles go
// with it under the same rule (deletesBundles): retention on, or SKILL_BUNDLE_SCOPE_RULES with SKILL_BUNDLES_ENABLED.
// With the scope rules or the bundle surface off and retention off, the bundles stay, as before (F-f03-08).
func TestPluginRemoveSkillRemovesBundlesAsTheSkillDeleteDoes(t *testing.T) {
	cases := []struct {
		name                                  string
		retention, scopeRules, bundlesEnabled bool
		want                                  bool
	}{
		{name: "scope rules and the bundle surface on", scopeRules: true, bundlesEnabled: true, want: true},
		{name: "scope rules off", scopeRules: false, bundlesEnabled: true, want: false},
		{name: "the bundle surface off", scopeRules: true, bundlesEnabled: false, want: false},
		{name: "both off", want: false},
		{name: "retention on", retention: true, want: true},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			skills := NewSkillsHandler(nil).WithBundleScopeRules(tc.scopeRules, tc.bundlesEnabled)
			skills.bundleRetention = tc.retention
			tx := &recordingDatabase{}
			store := newPluginChildStore(skills, nil)
			if err := store.RemoveSkill(context.Background(), tx, "org-1", "sk-1", "user-admin", "plugin removed"); err != nil {
				t.Fatalf("RemoveSkill: %v", err)
			}
			deletedSkill, removed := false, false
			for _, call := range tx.execs {
				if strings.Contains(call.query, "DELETE FROM agent_skills") {
					deletedSkill = true
				}
				if strings.Contains(call.query, "DELETE FROM skill_bundles") {
					removed = true
					if call.args[0] != "org-1" || call.args[1] != "sk-1" {
						t.Fatalf("bundle delete args = %#v, want the org and the skill", call.args)
					}
				}
			}
			if !deletedSkill {
				t.Fatal("RemoveSkill did not delete the skill row")
			}
			if removed != tc.want {
				t.Fatalf("bundles removed = %v, want %v", removed, tc.want)
			}
		})
	}
}

// A skill the plugin created but a person already deleted changes nothing: no bundle statement runs even with the
// bundle rule on.
func TestPluginRemoveSkillLeavesBundlesWhenTheSkillIsAlreadyGone(t *testing.T) {
	skills := NewSkillsHandler(nil).WithBundleScopeRules(true, true)
	tx := &recordingDatabase{execTag: "DELETE 0"}
	if err := newPluginChildStore(skills, nil).RemoveSkill(context.Background(), tx, "org-1", "sk-1", "user-admin", "plugin removed"); err != nil {
		t.Fatalf("RemoveSkill: %v", err)
	}
	for _, call := range tx.execs {
		if strings.Contains(call.query, "skill_bundles") {
			t.Fatalf("a bundle statement ran for a skill already gone: %s", call.query)
		}
	}
}
