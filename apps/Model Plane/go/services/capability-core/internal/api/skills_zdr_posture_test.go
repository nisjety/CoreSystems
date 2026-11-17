package api

import (
	"context"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
)

// W3-DEC-ZDRSKILL: a skill change is made under the posture of the verified principal. Only an explicit, verified
// non-ZDR posture keeps the skill text in the revision; every other posture, and no principal, writes a content-free
// revision.
func TestSkillRetentionPosture(t *testing.T) {
	zdr := authctx.Principal{OrganizationID: "org-a", ActorID: "user-a", PrincipalType: "user", ZeroDataRetention: true, RetentionPolicyPresent: true}
	unstated := authctx.Principal{OrganizationID: "org-a", ActorID: "user-a", PrincipalType: "user"}
	nonZDR := authctx.Principal{OrganizationID: "org-a", ActorID: "user-a", PrincipalType: "user", RetentionPolicyPresent: true}
	cases := []struct {
		name string
		ctx  context.Context
		want string
	}{
		{"zero data retention", authctx.ContextWithPrincipal(context.Background(), zdr), "true"},
		{"unstated posture", authctx.ContextWithPrincipal(context.Background(), unstated), "true"},
		{"explicit non-ZDR posture", authctx.ContextWithPrincipal(context.Background(), nonZDR), "false"},
		{"no verified principal", context.Background(), "true"},
	}
	for _, tc := range cases {
		if got := skillRetentionPosture(tc.ctx); got != tc.want {
			t.Errorf("%s: skillRetentionPosture = %q, want %q", tc.name, got, tc.want)
		}
	}
}

// A zero-retention caller may delete a skill (D20). The delete is made under the zero-retention posture, so its revision
// keeps no skill text; a non-ZDR caller's delete is made under the explicit non-ZDR posture.
func TestSkillDeleteUnderZDRWritesAContentFreeRevision(t *testing.T) {
	zdrToken := func(sign mcpTokenSigner) string {
		zdr := true
		return sign("user", []string{authz.WriteScope}, &zdr)
	}
	for _, tc := range []struct {
		name  string
		token func(sign mcpTokenSigner) string
		want  string
	}{
		{"zero-retention caller", zdrToken, "true"},
		{"explicit non-ZDR caller", writeToken, "false"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			database := &recordingDatabase{}
			response := serveSkill(t, database, tc.token, http.MethodDelete, "/api/v1/skills/sk-1", "")
			if response.Code != http.StatusNoContent {
				t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
			}
			if len(database.actorCalls) != 1 || len(database.actorCalls[0].args) != 5 {
				t.Fatalf("a delete names its actor once, with five settings: %#v", database.actorCalls)
			}
			if got := database.actorCalls[0].args[4]; got != tc.want {
				t.Fatalf("zdr posture = %v, want %q", got, tc.want)
			}
		})
	}
}

// The posture setting the registry writes is the one session-core's 0134 trigger reads. A rename on either side would
// turn every removal back into a snapshot of the skill text.
func TestSkillZDRPostureSettingMatchesMigration0134(t *testing.T) {
	migration, err := os.ReadFile(filepath.Join("..", "..", "..", "..", "..", "rust", "services", "session-core", "migrations", "0134_skill_revisions_zdr_content_free.sql"))
	if err != nil {
		t.Skipf("the session-core migration is not beside this module: %v", err)
	}
	if !strings.Contains(skillActorSQL, "set_config('mp.zdr_posture'") {
		t.Error("skillActorSQL does not set mp.zdr_posture")
	}
	if !strings.Contains(string(migration), "current_setting('mp.zdr_posture', true)") {
		t.Error("the 0134 trigger does not read mp.zdr_posture")
	}
}
