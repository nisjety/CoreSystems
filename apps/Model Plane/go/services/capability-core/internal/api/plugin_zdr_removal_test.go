package api

import (
	"context"
	"encoding/json"
	"net/http"
	"testing"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
	pluginloader "github.com/triodelab/model-plane/services/capability-core/internal/plugins"
)

// LEAD-FIX-GLOBALKILL-ZDR: RemoveSkill's posture follows the org it removes from. A global kill calls it once per
// affected org under one platform principal, so only the principal's own org may take the principal's posture. An
// org-scope kill and a person's own uninstall remove from the principal's own org and keep the posture unchanged.
func TestRemoveSkillPostureFollowsTheOrgBeingRemovedFrom(t *testing.T) {
	platform := authctx.Principal{
		OrganizationID: pluginGlobalOrg, ActorID: "service:platform-kill", PrincipalType: "service",
		Scopes: []string{authz.WriteScope, authz.GlobalWriteScope}, RetentionPolicyPresent: true,
	}
	nonZDRAdmin := pluginWriterPrincipal()
	zdrAdmin := pluginWriterPrincipal()
	zdrAdmin.ZeroDataRetention = true
	unstatedAdmin := pluginWriterPrincipal()
	unstatedAdmin.RetentionPolicyPresent = false

	cases := []struct {
		name      string
		principal *authctx.Principal
		orgID     string
		want      string
	}{
		{"global kill: the platform's explicit non-ZDR posture does not reach a ZDR org", &platform, "org-zdr", "true"},
		{"global kill: the platform's explicit non-ZDR posture does not reach a non-ZDR org", &platform, "org-b", "true"},
		{"global kill: the platform's own org keeps its own verified non-ZDR posture", &platform, pluginGlobalOrg, "false"},
		{"org-scope kill by a non-ZDR admin keeps the verified posture", &nonZDRAdmin, "org-a", "false"},
		{"own uninstall by a ZDR admin is content-free", &zdrAdmin, "org-a", "true"},
		{"own uninstall with an unstated posture is content-free", &unstatedAdmin, "org-a", "true"},
		{"no verified principal is content-free", nil, "org-a", "true"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			ctx := context.Background()
			if tc.principal != nil {
				ctx = authctx.ContextWithPrincipal(ctx, *tc.principal)
			}
			db := &recordingDatabase{}
			store := newPluginChildStore(&SkillsHandler{}, nil)
			if err := store.RemoveSkill(ctx, db, tc.orgID, "skill-1", "actor-1", "plugin_removed:plugin-1"); err != nil {
				t.Fatalf("RemoveSkill: %v", err)
			}
			if len(db.execs) != 2 || db.execs[0].query != skillActorSQL || db.execs[1].args[1] != tc.orgID {
				t.Fatalf("want the actor statement, then the delete of the org's own skill: %#v", db.execs)
			}
			if got := db.execs[0].args[4]; got != tc.want {
				t.Fatalf("zdr posture = %v, want %q", got, tc.want)
			}
		})
	}
}

// W3-DEC-ZDRSKILL: an uninstall removes a plugin's skill under the admin's own posture, through the loader that the
// install and the kill share. A zero-retention admin's removal is content-free; a non-ZDR admin's keeps its posture.
func TestUninstallRemovesSkillUnderTheAdminsOwnPosture(t *testing.T) {
	for _, tc := range []struct {
		name string
		zdr  bool
		want string
	}{
		{"zero-retention admin", true, "true"},
		{"non-ZDR admin", false, "false"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			admin := pluginWriterPrincipal()
			admin.ZeroDataRetention = tc.zdr
			ctx := authctx.ContextWithPrincipal(context.Background(), admin)
			db := &killFakeDB{components: map[string][][]any{
				"plugin-a": {{pluginloader.KindSkill, "triage", "skill-1"}},
			}}
			tx, err := db.Begin(ctx)
			if err != nil {
				t.Fatal(err)
			}
			loader := &pluginloader.Loader{Children: newPluginChildStore(&SkillsHandler{}, nil)}
			plugin := pluginloader.Plugin{
				ID: "plugin-a", OrgID: admin.OrganizationID, Name: "research-kit", Version: "1.0.0",
				ManifestDigest: goodArchive, Installed: true,
			}
			caller := pluginloader.Caller{
				ID: admin.ActorID, IsPerson: true, IsAdmin: true,
				ZeroDataRetention: tc.zdr, RetentionPolicyKnown: true,
			}
			if _, err := loader.Uninstall(ctx, tx, plugin, caller, pluginloader.ReasonUninstall); err != nil {
				t.Fatalf("uninstall: %v", err)
			}
			removals := skillRemovals(t, db.execs)
			if len(removals) != 1 || removals[0].id != "skill-1" || removals[0].orgID != admin.OrganizationID || removals[0].posture != tc.want {
				t.Fatalf("removals = %#v, want one skill-1 removal in the admin's org with posture %q", removals, tc.want)
			}
		})
	}
}

// W3-DEC-ZDRDEL3: a zero-retention admin may lift a kill (DELETE plugin-kill-list), which removes one entry and whose
// audit row names only the digest and the scope. The rest of the catalogue and kill-list writes stay refused under zero
// retention, and an unstated posture may not lift a kill either.
func TestZDRAdminMayLiftAKillButNotWriteTheCatalogue(t *testing.T) {
	zdr := pluginWriterPrincipal()
	zdr.ZeroDataRetention = true
	unstated := pluginWriterPrincipal()
	unstated.RetentionPolicyPresent = false

	db := &pluginFakeDB{affected: 1}
	handler := NewPluginCatalogHandler(db).WithLoaderEnabled(true).WithChildStores(nil, nil)
	recorder := servePlugins(handler, zdr, http.MethodDelete, "/api/v1/plugin-kill-list?digest="+goodArchive+"&scope=org", "")
	if recorder.Code != http.StatusOK || len(db.execs) != 1 {
		t.Fatalf("zdr lift status = %d, writes = %d, body = %s", recorder.Code, len(db.execs), recorder.Body.String())
	}
	var lifted struct {
		Removed bool `json:"removed"`
	}
	if err := json.Unmarshal(recorder.Body.Bytes(), &lifted); err != nil || !lifted.Removed {
		t.Fatalf("zdr lift answer = %s, want removed true", recorder.Body.String())
	}

	refused := []struct {
		name      string
		principal authctx.Principal
		method    string
		target    string
		body      string
	}{
		{"zdr admin kills", zdr, http.MethodPost, "/api/v1/plugin-kill-list", `{"digest":"` + goodArchive + `"}`},
		{"zdr admin approves", zdr, http.MethodPost, "/api/v1/plugin-catalog/plugin_1/approve", ``},
		{"unstated posture lifts", unstated, http.MethodDelete, "/api/v1/plugin-kill-list?digest=" + goodArchive + "&scope=org", ``},
	}
	for _, tc := range refused {
		t.Run(tc.name, func(t *testing.T) {
			db := &pluginFakeDB{affected: 1}
			handler := NewPluginCatalogHandler(db).WithLoaderEnabled(true).WithChildStores(nil, nil)
			recorder := servePlugins(handler, tc.principal, tc.method, tc.target, tc.body)
			if recorder.Code != http.StatusForbidden || typedCode(t, recorder) != pluginWriteZDRCode || len(db.execs) != 0 {
				t.Fatalf("status=%d code=%q writes=%d, want 403 %s and no write",
					recorder.Code, typedCode(t, recorder), len(db.execs), pluginWriteZDRCode)
			}
		})
	}
}
