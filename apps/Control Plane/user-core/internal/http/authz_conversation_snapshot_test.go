package http

import (
	"encoding/json"
	"net/http"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/authztaxonomy"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/users"
)

// W4-SH-3 (pure tier): session-core is the one owner service of
// conversation_snapshot share grants. It writes them in owner-asserted mode,
// view only, never on a Space, and it can never write another owner's type
// (nor can the other owner write snapshot grants). The read-time membership
// gate on /authz/check needs the database and is not covered here.

const snapshotGrantBody = `{"org_id":"org-1","resource_type":"conversation_snapshot","resource_id":"snap-1","subject_id":"user-9","role":"view"}`

func sessionCoreSnapshotDelegation(operation string) map[string]any {
	return with(ownerDelegation(operation, "conversation_snapshot", "snap-1"), map[string]any{"service_id": "session-core"})
}

func TestConversationSnapshotOwnerIsSessionCoreOnly(t *testing.T) {
	owner, ok := authztaxonomy.OwnerService("conversation_snapshot")
	if !ok || owner != "session-core" {
		t.Fatalf("OwnerService(conversation_snapshot) = %q, %v; want session-core", owner, ok)
	}
	if authztaxonomy.SpaceGrantable("conversation_snapshot") {
		t.Fatal("conversation_snapshot must never be Space-grantable")
	}
	if authztaxonomy.EditEnforced("conversation_snapshot") {
		t.Fatal("conversation_snapshot is view only; edit must not be enforced")
	}
}

func TestConversationSnapshotGrantsAreFencedWhenTheFlagIsOff(t *testing.T) {
	s := &Server{} // CONTROL_OWNER_ASSERTED_GRANTS unset
	engine, probe := authzEngine(sessionCoreSnapshotDelegation("authz:grant"), s, nil)
	rec := authzDo(engine, http.MethodPost, authzGrantPath, snapshotGrantBody)
	if rec.Code != http.StatusForbidden || probe.reached {
		t.Fatalf("flag off: %d %s reached=%v", rec.Code, rec.Body.String(), probe.reached)
	}
}

func TestConversationSnapshotGrantsAdmitSessionCoreForEveryGrantOperation(t *testing.T) {
	s := &Server{}
	s.SetOwnerAssertedGrants(true)
	for name, tc := range map[string]struct {
		method, target, body, operation string
	}{
		"grant":   {http.MethodPost, authzGrantPath, snapshotGrantBody, "authz:grant"},
		"revoke":  {http.MethodDelete, authzGrantPath + "?org_id=org-1&resource_type=conversation_snapshot&resource_id=snap-1&subject_id=user-9", "", "authz:revoke"},
		"listing": {http.MethodGet, authzGrantsPath + "?org_id=org-1&resource_type=conversation_snapshot&resource_id=snap-1", "", "authz:grants"},
	} {
		t.Run(name, func(t *testing.T) {
			engine, probe := authzEngine(sessionCoreSnapshotDelegation(tc.operation), s, nil)
			rec := authzDo(engine, tc.method, tc.target, tc.body)
			if rec.Code != http.StatusOK || !probe.reached || !probe.marked {
				t.Fatalf("%d %s reached=%v marked=%v", rec.Code, rec.Body.String(), probe.reached, probe.marked)
			}
		})
	}
}

func TestConversationSnapshotAndDocumentOwnersCannotCrossOver(t *testing.T) {
	s := &Server{}
	s.SetOwnerAssertedGrants(true)
	for name, tc := range map[string]struct {
		ctx  map[string]any
		body string
	}{
		"documents-api writes a snapshot grant": {
			with(ownerDelegation("authz:grant", "conversation_snapshot", "snap-1"), map[string]any{"service_id": documentsAPIPrincipal}),
			snapshotGrantBody,
		},
		"session-core writes a document grant": {
			with(ownerDelegation("authz:grant", "document", "doc-1"), map[string]any{"service_id": "session-core"}),
			grantBody,
		},
		"another Model Plane service writes a snapshot grant": {
			with(ownerDelegation("authz:grant", "conversation_snapshot", "snap-1"), map[string]any{"service_id": "model-gateway"}),
			snapshotGrantBody,
		},
	} {
		t.Run(name, func(t *testing.T) {
			engine, probe := authzEngine(tc.ctx, s, nil)
			rec := authzDo(engine, http.MethodPost, authzGrantPath, tc.body)
			if rec.Code != http.StatusForbidden || probe.reached {
				t.Fatalf("%d %s reached=%v", rec.Code, rec.Body.String(), probe.reached)
			}
			if got := errorOf(t, rec); got != "registered resource owner service required" {
				t.Fatalf("error = %q", got)
			}
		})
	}
}

func TestConversationSnapshotGrantsAreViewOnlyAndUserSubjectOnly(t *testing.T) {
	s := &Server{aclRepo: &users.AclRepository{}} // every refusal happens before any query
	s.SetOwnerAssertedGrants(true)
	handler := func(c *gin.Context) { s.authzGrant(c) }
	for name, tc := range map[string]struct {
		body string
		code string
	}{
		"edit role":     {strings.Replace(snapshotGrantBody, `"role":"view"`, `"role":"edit"`, 1), "role_not_supported"},
		"owner role":    {strings.Replace(snapshotGrantBody, `"role":"view"`, `"role":"owner"`, 1), "role_not_supported"},
		"space subject": {strings.Replace(snapshotGrantBody, `"role":"view"`, `"role":"view","subject_type":"space"`, 1), "subject_type_not_supported"},
		"team subject":  {strings.Replace(snapshotGrantBody, `"role":"view"`, `"role":"view","subject_type":"team"`, 1), "subject_type_not_supported"},
	} {
		t.Run(name, func(t *testing.T) {
			engine, _ := authzEngine(sessionCoreSnapshotDelegation("authz:grant"), s, handler)
			rec := authzDo(engine, http.MethodPost, authzGrantPath, tc.body)
			if rec.Code != http.StatusBadRequest {
				t.Fatalf("status = %d, want 400 (%s)", rec.Code, rec.Body.String())
			}
			var decoded map[string]any
			_ = json.Unmarshal(rec.Body.Bytes(), &decoded)
			if decoded["code"] != tc.code {
				t.Fatalf("code = %v, want %s (%s)", decoded["code"], tc.code, rec.Body.String())
			}
		})
	}
}
