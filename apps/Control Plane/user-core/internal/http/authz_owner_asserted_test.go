package http

import (
	"bytes"
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/users"
)

// WS14-S2 (pure tier): owner-asserted grants, the Space-visible read and the
// room-file grant route. Everything that needs the database (granted_by, the
// org-member check, the file-grant authority re-resolution and replay) is in
// authz_owner_asserted_live_test.go.

// ownerDelegation is what the service-auth layer would have set for a
// documents-api call with a fully verified v2 + zdr delegation.
func ownerDelegation(operation, resourceType, resourceID string) map[string]any {
	return map[string]any{
		"auth_method": "service_principal", "service_id": documentsAPIPrincipal,
		"delegation_verified": true, "delegated_user_proof_verified": true,
		"delegation_version": "v2", "delegation_zdr": "true",
		"delegation_operation": operation, "delegation_resource_type": resourceType, "delegation_resource_id": resourceID,
		"org_id": "org-1", "user_id": "owner-1",
	}
}

func with(ctx map[string]any, overrides map[string]any) map[string]any {
	out := make(map[string]any, len(ctx)+len(overrides))
	for k, v := range ctx {
		out[k] = v
	}
	for k, v := range overrides {
		if v == nil {
			delete(out, k)
		} else {
			out[k] = v
		}
	}
	return out
}

// authzEngine mounts the real guard (requireVerifiedAuthzDelegation) in front
// of a probe that records whether the chain was admitted and whether the
// owner-asserted marker was set. A handler can be passed instead of the probe.
type authzProbe struct {
	reached bool
	marked  bool
}

func authzEngine(ctx map[string]any, s *Server, handler gin.HandlerFunc) (*gin.Engine, *authzProbe) {
	gin.SetMode(gin.TestMode)
	probe := &authzProbe{}
	engine := gin.New()
	setCtx := func(c *gin.Context) {
		for k, v := range ctx {
			c.Set(k, v)
		}
		c.Next()
	}
	final := handler
	if final == nil {
		final = func(c *gin.Context) {
			probe.reached = true
			probe.marked = c.GetBool(ownerAssertedContextKey)
			c.JSON(http.StatusOK, gin.H{"ok": true})
		}
	}
	for _, route := range []struct{ method, path string }{
		{http.MethodPost, authzGrantPath}, {http.MethodDelete, authzGrantPath},
		{http.MethodGet, authzGrantsPath}, {http.MethodGet, authzSpaceVisiblePath},
	} {
		engine.Handle(route.method, route.path, setCtx, s.requireVerifiedAuthzDelegation, final)
	}
	return engine, probe
}

func authzDo(engine *gin.Engine, method, target, body string) *httptest.ResponseRecorder {
	req := httptest.NewRequest(method, target, strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()
	engine.ServeHTTP(rec, req)
	return rec
}

const grantBody = `{"org_id":"org-1","resource_type":"document","resource_id":"doc-1","subject_id":"user-9","role":"view"}`

func errorOf(t *testing.T, rec *httptest.ResponseRecorder) string {
	t.Helper()
	var body map[string]any
	_ = json.Unmarshal(rec.Body.Bytes(), &body)
	message, _ := body["error"].(string)
	return message
}

func TestOwnerAssertedFlagOffKeepsTheFenceForEveryCaller(t *testing.T) {
	s := &Server{} // CONTROL_OWNER_ASSERTED_GRANTS unset
	for name, tc := range map[string]struct {
		method, target, body, operation string
	}{
		"grant":   {http.MethodPost, authzGrantPath, grantBody, "authz:grant"},
		"revoke":  {http.MethodDelete, authzGrantPath + "?org_id=org-1&resource_type=document&resource_id=doc-1&subject_id=user-9", "", "authz:revoke"},
		"listing": {http.MethodGet, authzGrantsPath + "?org_id=org-1&resource_type=document&resource_id=doc-1", "", "authz:grants"},
	} {
		t.Run(name, func(t *testing.T) {
			engine, probe := authzEngine(ownerDelegation(tc.operation, "document", "doc-1"), s, nil)
			rec := authzDo(engine, tc.method, tc.target, tc.body)
			if rec.Code != http.StatusForbidden || errorOf(t, rec) != "verified resource-owner delegation required" || probe.reached {
				t.Fatalf("flag off: %d %s reached=%v", rec.Code, rec.Body.String(), probe.reached)
			}
		})
	}
}

func TestOwnerAssertedAdmitsOnlyTheRegisteredOwnerService(t *testing.T) {
	s := &Server{}
	s.SetOwnerAssertedGrants(true)
	base := ownerDelegation("authz:grant", "document", "doc-1")

	// documents-api, fully delegated: admitted and marked.
	engine, probe := authzEngine(base, s, nil)
	if rec := authzDo(engine, http.MethodPost, authzGrantPath, grantBody); rec.Code != http.StatusOK || !probe.reached || !probe.marked {
		t.Fatalf("documents-api: %d %s reached=%v marked=%v", rec.Code, rec.Body.String(), probe.reached, probe.marked)
	}

	for name, ctx := range map[string]map[string]any{
		"gateway principal":    with(base, map[string]any{"service_id": verevonGatewayPrincipal}),
		"another service":      with(base, map[string]any{"service_id": "ingestion-core"}),
		"no service id":        with(base, map[string]any{"service_id": nil}),
		"unregistered type":    ownerDelegation("authz:grant", "inbox", "doc-1"),
		"decision-only caller": with(base, map[string]any{"service_id": "orchestrator-core"}),
	} {
		t.Run(name, func(t *testing.T) {
			engine, probe := authzEngine(ctx, s, nil)
			body := grantBody
			if name == "unregistered type" {
				body = strings.Replace(grantBody, `"document"`, `"inbox"`, 1)
			}
			rec := authzDo(engine, http.MethodPost, authzGrantPath, body)
			if rec.Code != http.StatusForbidden || probe.reached {
				t.Fatalf("%d %s reached=%v", rec.Code, rec.Body.String(), probe.reached)
			}
			if got := errorOf(t, rec); got != "registered resource owner service required" {
				t.Fatalf("error = %q", got)
			}
		})
	}
}

func TestOwnerAssertedNeedsAV2ZdrBoundDelegationAndAUserProof(t *testing.T) {
	s := &Server{}
	s.SetOwnerAssertedGrants(true)
	base := ownerDelegation("authz:grant", "document", "doc-1")
	for name, ctx := range map[string]map[string]any{
		"delegation not verified": with(base, map[string]any{"delegation_verified": false}),
		"no user proof":           with(base, map[string]any{"delegated_user_proof_verified": false}),
		"bearer caller":           with(base, map[string]any{"auth_method": "bearer"}),
		"v1 delegation":           with(base, map[string]any{"delegation_version": "v1"}),
		"zdr claim absent":        with(base, map[string]any{"delegation_zdr": ""}),
		"zdr claim false":         with(base, map[string]any{"delegation_zdr": "false"}),
	} {
		t.Run(name, func(t *testing.T) {
			engine, probe := authzEngine(ctx, s, nil)
			rec := authzDo(engine, http.MethodPost, authzGrantPath, grantBody)
			if rec.Code != http.StatusForbidden || probe.reached {
				t.Fatalf("%d %s reached=%v", rec.Code, rec.Body.String(), probe.reached)
			}
		})
	}
}

func TestOwnerAssertedDelegationMustMatchOperationResourceAndTenant(t *testing.T) {
	s := &Server{}
	s.SetOwnerAssertedGrants(true)
	cases := []struct {
		name    string
		ctx     map[string]any
		method  string
		target  string
		body    string
		wantErr string
	}{
		{"revoke delegation used to grant", ownerDelegation("authz:revoke", "document", "doc-1"), http.MethodPost, authzGrantPath, grantBody, "delegated operation mismatch"},
		{"grant delegation used to list", ownerDelegation("authz:grant", "document", "doc-1"), http.MethodGet, authzGrantsPath + "?org_id=org-1&resource_type=document&resource_id=doc-1", "", "delegated operation mismatch"},
		{"other document", ownerDelegation("authz:grant", "document", "doc-OTHER"), http.MethodPost, authzGrantPath, grantBody, "delegated resource mismatch"},
		{"other resource type", ownerDelegation("authz:grant", "inbox", "doc-1"), http.MethodPost, authzGrantPath, grantBody, "delegated resource mismatch"},
		{"missing resource id in body", ownerDelegation("authz:grant", "document", "doc-1"), http.MethodPost, authzGrantPath,
			`{"org_id":"org-1","resource_type":"document","subject_id":"user-9"}`, "delegated resource mismatch"},
		{"other tenant in body", ownerDelegation("authz:grant", "document", "doc-1"), http.MethodPost, authzGrantPath,
			strings.Replace(grantBody, "org-1", "org-2", 1), "delegated tenant mismatch"},
		{"missing tenant in body", ownerDelegation("authz:grant", "document", "doc-1"), http.MethodPost, authzGrantPath,
			`{"resource_type":"document","resource_id":"doc-1","subject_id":"user-9"}`, "delegated tenant mismatch"},
		{"other tenant in query", ownerDelegation("authz:grants", "document", "doc-1"), http.MethodGet,
			authzGrantsPath + "?org_id=org-2&resource_type=document&resource_id=doc-1", "", "delegated tenant mismatch"},
		{"other document in query", ownerDelegation("authz:grants", "document", "doc-1"), http.MethodGet,
			authzGrantsPath + "?org_id=org-1&resource_type=document&resource_id=doc-2", "", "delegated resource mismatch"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			engine, probe := authzEngine(tc.ctx, s, nil)
			rec := authzDo(engine, tc.method, tc.target, tc.body)
			if rec.Code != http.StatusForbidden || probe.reached {
				t.Fatalf("%d %s reached=%v", rec.Code, rec.Body.String(), probe.reached)
			}
			if got := errorOf(t, rec); got != tc.wantErr {
				t.Fatalf("error = %q, want %q", got, tc.wantErr)
			}
		})
	}

	// A malformed body is a 400, not a pass-through.
	engine, probe := authzEngine(ownerDelegation("authz:grant", "document", "doc-1"), s, nil)
	if rec := authzDo(engine, http.MethodPost, authzGrantPath, "not json"); rec.Code != http.StatusBadRequest || probe.reached {
		t.Fatalf("malformed body: %d reached=%v", rec.Code, probe.reached)
	}

	// The guard re-arms the body for the handler.
	var seen string
	engine, _ = authzEngine(ownerDelegation("authz:grant", "document", "doc-1"), s, func(c *gin.Context) {
		var buf bytes.Buffer
		_, _ = buf.ReadFrom(c.Request.Body)
		seen = buf.String()
		c.Status(http.StatusNoContent)
	})
	authzDo(engine, http.MethodPost, authzGrantPath, grantBody)
	if seen != grantBody {
		t.Fatalf("handler body = %q, want the original", seen)
	}
}

func TestOwnerAssertedWritesRefuseRolesAndSubjectsThatPromiseTooMuch(t *testing.T) {
	s := &Server{aclRepo: &users.AclRepository{}} // the refusals happen before any query
	s.SetOwnerAssertedGrants(true)
	handler := func(c *gin.Context) { s.authzGrant(c) }
	for name, tc := range map[string]struct {
		patch string
		code  string
	}{
		"edit role (stored but unenforced)": {`"role":"edit"`, "role_not_supported"},
		"unknown role":                      {`"role":"admin"`, "role_not_supported"},
		"owner role":                        {`"role":"owner"`, "role_not_supported"},
		"space subject on POST":             {`"subject_type":"space"`, "subject_type_not_supported"},
		"team subject":                      {`"subject_type":"team"`, "subject_type_not_supported"},
	} {
		t.Run(name, func(t *testing.T) {
			engine, _ := authzEngine(ownerDelegation("authz:grant", "document", "doc-1"), s, handler)
			body := strings.Replace(grantBody, `"role":"view"`, tc.patch, 1)
			if strings.HasPrefix(tc.patch, `"subject_type"`) {
				body = strings.Replace(grantBody, `"role":"view"`, `"role":"view",`+tc.patch, 1)
			}
			rec := authzDo(engine, http.MethodPost, authzGrantPath, body)
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

func TestSpaceVisibleIsDarkWhenTheFlagIsOffAndPinsEveryClaimWhenOn(t *testing.T) {
	const target = authzSpaceVisiblePath + "?org_id=org-1&subject_id=owner-1&resource_type=document&space_ref=room-1"
	base := ownerDelegation("authz:space-visible", "document", "room-1")

	off := &Server{}
	engine, probe := authzEngine(base, off, nil)
	if rec := authzDo(engine, http.MethodGet, target, ""); rec.Code != http.StatusNotFound || probe.reached {
		t.Fatalf("flag off: %d %s reached=%v", rec.Code, rec.Body.String(), probe.reached)
	}

	on := &Server{}
	on.SetSpaceSubjectGrants(true)
	engine, probe = authzEngine(base, on, nil)
	if rec := authzDo(engine, http.MethodGet, target, ""); rec.Code != http.StatusOK || !probe.reached {
		t.Fatalf("matching delegation: %d %s", rec.Code, rec.Body.String())
	}

	cases := map[string]struct {
		ctx    map[string]any
		target string
	}{
		"other operation":       {ownerDelegation("authz:visible", "document", "room-1"), target},
		"other resource type":   {ownerDelegation("authz:space-visible", "inbox", "room-1"), target},
		"other space":           {ownerDelegation("authz:space-visible", "document", "room-OTHER"), target},
		"other tenant":          {base, strings.Replace(target, "org_id=org-1", "org_id=org-2", 1)},
		"other subject":         {base, strings.Replace(target, "subject_id=owner-1", "subject_id=someone-else", 1)},
		"no space ref":          {base, strings.Replace(target, "&space_ref=room-1", "", 1)},
		"v1 delegation":         {with(base, map[string]any{"delegation_version": "v1"}), target},
		"unverified delegation": {with(base, map[string]any{"delegation_verified": false}), target},
	}
	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			engine, probe := authzEngine(tc.ctx, on, nil)
			rec := authzDo(engine, http.MethodGet, tc.target, "")
			if rec.Code != http.StatusForbidden || probe.reached {
				t.Fatalf("%d %s reached=%v", rec.Code, rec.Body.String(), probe.reached)
			}
		})
	}
}

func TestSpaceVisibleHandlerRefusesNonAttachableTypesAndFailsClosedWithoutStorage(t *testing.T) {
	s := &Server{}
	s.SetSpaceSubjectGrants(true)
	handler := func(c *gin.Context) { s.authzSpaceVisible(c) }

	engine, _ := authzEngine(ownerDelegation("authz:space-visible", "inbox", "room-1"), s, handler)
	rec := authzDo(engine, http.MethodGet, authzSpaceVisiblePath+"?org_id=org-1&subject_id=owner-1&resource_type=inbox&space_ref=room-1", "")
	if rec.Code != http.StatusBadRequest || !strings.Contains(rec.Body.String(), "resource_type_not_supported") {
		t.Fatalf("non-attachable type: %d %s", rec.Code, rec.Body.String())
	}

	engine, _ = authzEngine(ownerDelegation("authz:space-visible", "document", "room-1"), s, handler)
	rec = authzDo(engine, http.MethodGet, authzSpaceVisiblePath+"?org_id=org-1&subject_id=owner-1&resource_type=document&space_ref=room-1", "")
	if rec.Code != http.StatusServiceUnavailable {
		t.Fatalf("unwired repository: %d %s", rec.Code, rec.Body.String())
	}
}

// ---- file-grant ----------------------------------------------------------

const fileGrantPath = "/api/v1/internal/spaces/file-grant"

func documentsAPIContext() map[string]any {
	return map[string]any{
		"auth_method": "service_principal", "service_id": documentsAPIPrincipal,
		"service_scopes": []string{scopeSpaceFileGrant}, "org_id": "org-1",
	}
}

// fileGrantEngine mounts the real guard and handler.
func fileGrantEngine(ctx map[string]any, s *Server) *gin.Engine {
	gin.SetMode(gin.TestMode)
	engine := gin.New()
	engine.POST(fileGrantPath, func(c *gin.Context) {
		for k, v := range ctx {
			c.Set(k, v)
		}
		c.Next()
	}, s.requireSpaceFileGrantWriter, s.issueSpaceFileGrant)
	return engine
}

func fileGrantServer(flag bool, withRepos bool) *Server {
	s := &Server{}
	s.SetSpaceSubjectGrants(flag)
	if withRepos {
		s.spaceRepo = &spaces.Repository{}
		s.aclRepo = &users.AclRepository{}
	}
	return s
}

func signFileGrantToken(t *testing.T, mutate func(*spaces.Decision), signWith *spaces.SigningKey) string {
	t.Helper()
	key, err := spaces.LoadSigningKeyFromEnv(os.Getenv)
	if err != nil {
		t.Fatalf("signing key: %v", err)
	}
	if signWith != nil {
		key = *signWith
	}
	now := time.Now().UTC()
	decision := spaces.Decision{
		DecisionRef: "decision-1", OrgID: "org-1", SpaceRef: "room-1", SubjectID: "owner-1",
		ServiceAudience: "data-plane-import", ActionID: "ingestion.import.write", ActionSchemaHash: "sha256:ingestion-import-v1",
		PayloadDigest: "sha256:payload-1", IdempotencyKey: "import-1",
		RecipientAudienceRef: "audience:room-1:1", RecipientAudienceHash: "sha256:audience-1",
		PrivacyPolicyRef: "privacy:org-1:1", ResourceAuthorizationRef: "control:room-1:import:1",
		AuthorityRevision: 7, MembershipRevision: 4, PrivacyRevision: 5, RecipientAudienceRevision: 2, EntitlementRevision: 3,
		Permissions: []string{"documents:write"}, Purpose: "assistant_collaboration", LawfulBasis: "contract",
		PrivacyClass: "internal", RetentionClass: "standard", Residency: "swedencentral", DeletionScope: "space",
		ImportSourceType: spaces.SpaceUploadSourceType,
		IssuedAt:         now.Add(-time.Minute), ExpiresAt: now.Add(time.Minute), Nonce: "nonce-1",
	}
	if mutate != nil {
		mutate(&decision)
	}
	token, err := spaces.SignDecision(key, decision)
	if err != nil {
		t.Fatalf("sign: %v", err)
	}
	return token
}

func fileGrantBody(token string) string {
	b, _ := json.Marshal(map[string]string{"decision_token": token, "document_id": "doc-1"})
	return string(b)
}

func setFileGrantKey(t *testing.T) spaces.SigningKey {
	t.Helper()
	seed := bytes.Repeat([]byte{0x3c}, ed25519.SeedSize)
	t.Setenv("CONTROL_SPACE_DECISION_KEY_ID", "ws14s2-test-key")
	t.Setenv("CONTROL_SPACE_DECISION_PRIVATE_KEY_BASE64", base64.RawURLEncoding.EncodeToString(seed))
	key, err := spaces.LoadSigningKeyFromEnv(os.Getenv)
	if err != nil {
		t.Fatal(err)
	}
	return key
}

func TestFileGrantScopeRuleIsNotADelegatedScope(t *testing.T) {
	scope := serviceScopeForRequest(httptest.NewRequest(http.MethodPost, fileGrantPath, nil))
	if scope != "spaces:file-grant:write" {
		t.Fatalf("scope = %q", scope)
	}
	if serviceScopeRequiresVerifiedDelegation(scope) {
		t.Fatal("the import worker has no user bearer; the scope must not demand a delegation")
	}
	// A trailing slash maps the same; a GET does not.
	if got := serviceScopeForRequest(httptest.NewRequest(http.MethodPost, fileGrantPath+"/", nil)); got != scopeSpaceFileGrant {
		t.Fatalf("trailing slash scope = %q", got)
	}
	if got := serviceScopeForRequest(httptest.NewRequest(http.MethodGet, fileGrantPath, nil)); got == scopeSpaceFileGrant {
		t.Fatal("a GET must not map to the file-grant write scope")
	}
}

func TestFileGrantIsDarkWhenTheFlagIsOffAndAdmitsOnlyDocumentsAPI(t *testing.T) {
	engine := fileGrantEngine(documentsAPIContext(), fileGrantServer(false, true))
	if rec := authzDo(engine, http.MethodPost, fileGrantPath, `{"decision_token":"x","document_id":"d"}`); rec.Code != http.StatusNotFound {
		t.Fatalf("flag off: %d %s", rec.Code, rec.Body.String())
	}

	on := fileGrantServer(true, true)
	for name, ctx := range map[string]map[string]any{
		"gateway":         with(documentsAPIContext(), map[string]any{"service_id": verevonGatewayPrincipal}),
		"another service": with(documentsAPIContext(), map[string]any{"service_id": "ingestion-core"}),
		"missing scope":   with(documentsAPIContext(), map[string]any{"service_scopes": []string{"spaces:issue"}}),
		"no scopes":       with(documentsAPIContext(), map[string]any{"service_scopes": nil}),
		"user bearer":     with(documentsAPIContext(), map[string]any{"auth_method": "bearer"}),
	} {
		t.Run(name, func(t *testing.T) {
			rec := authzDo(fileGrantEngine(ctx, on), http.MethodPost, fileGrantPath, `{"decision_token":"x","document_id":"d"}`)
			if rec.Code != http.StatusForbidden {
				t.Fatalf("%d %s", rec.Code, rec.Body.String())
			}
		})
	}
}

func TestFileGrantRejectsMalformedBodiesBeforeTouchingAnything(t *testing.T) {
	engine := fileGrantEngine(documentsAPIContext(), fileGrantServer(true, true))
	for name, body := range map[string]string{
		"empty":            ``,
		"not json":         `nope`,
		"no token":         `{"document_id":"d"}`,
		"no document":      `{"decision_token":"t"}`,
		"blank values":     `{"decision_token":" ","document_id":" "}`,
		"unknown field":    `{"decision_token":"t","document_id":"d","org_id":"org-9"}`,
		"trailing garbage": `{"decision_token":"t","document_id":"d"} {}`,
		"long document id": `{"decision_token":"t","document_id":"` + strings.Repeat("d", fileGrantMaxIDLen+1) + `"}`,
	} {
		t.Run(name, func(t *testing.T) {
			rec := authzDo(engine, http.MethodPost, fileGrantPath, body)
			if rec.Code != http.StatusBadRequest || !strings.Contains(rec.Body.String(), "invalid_request") {
				t.Fatalf("%d %s", rec.Code, rec.Body.String())
			}
		})
	}
}

func TestFileGrantFailsClosedWithoutRepositoriesOrSigningKey(t *testing.T) {
	body := `{"decision_token":"t","document_id":"d"}`
	rec := authzDo(fileGrantEngine(documentsAPIContext(), fileGrantServer(true, false)), http.MethodPost, fileGrantPath, body)
	if rec.Code != http.StatusServiceUnavailable {
		t.Fatalf("no repositories: %d %s", rec.Code, rec.Body.String())
	}
	t.Setenv("CONTROL_SPACE_DECISION_KEY_ID", "")
	t.Setenv("CONTROL_SPACE_DECISION_PRIVATE_KEY_BASE64", "")
	rec = authzDo(fileGrantEngine(documentsAPIContext(), fileGrantServer(true, true)), http.MethodPost, fileGrantPath, body)
	if rec.Code != http.StatusServiceUnavailable {
		t.Fatalf("no signing key: %d %s", rec.Code, rec.Body.String())
	}
}

func TestFileGrantRefusesBadForeignAndExpiredTokensAndWrongDecisionShapes(t *testing.T) {
	key := setFileGrantKey(t)
	engine := fileGrantEngine(documentsAPIContext(), fileGrantServer(true, true))

	foreignPublic, foreignPrivate, err := ed25519.GenerateKey(nil)
	_ = foreignPublic
	if err != nil {
		t.Fatal(err)
	}
	foreign := spaces.SigningKey{ID: key.ID, PrivateKey: foreignPrivate}
	good := signFileGrantToken(t, nil, nil)
	parts := strings.Split(good, ".")
	tamperedSource := signFileGrantToken(t, func(d *spaces.Decision) { d.SubjectID = "intruder" }, nil)

	cases := map[string]struct {
		token string
		code  string
	}{
		"garbage":               {"not-a-token", "decision_invalid"},
		"truncated":             {strings.Join(parts[:3], "."), "decision_invalid"},
		"tampered payload":      {strings.Join([]string{parts[0], parts[1], strings.Split(tamperedSource, ".")[2], parts[3]}, "."), "decision_invalid"},
		"signed by another key": {signFileGrantToken(t, nil, &foreign), "decision_invalid"},
		"expired": {signFileGrantToken(t, func(d *spaces.Decision) {
			d.IssuedAt = time.Now().UTC().Add(-10 * time.Minute)
			d.ExpiresAt = time.Now().UTC().Add(-5 * time.Minute)
		}, nil), "decision_invalid"},
		"wrong action": {signFileGrantToken(t, func(d *spaces.Decision) {
			d.ActionID = "model.thread.create"
			d.ActionSchemaHash = "sha256:thread-create-v1"
		}, nil), "decision_invalid"},
		"initiation audience":  {signFileGrantToken(t, func(d *spaces.Decision) { d.ServiceAudience = "imports-core" }, nil), "decision_invalid"},
		"model plane audience": {signFileGrantToken(t, func(d *spaces.Decision) { d.ServiceAudience = "model-plane" }, nil), "decision_invalid"},
		"personal source type": {signFileGrantToken(t, func(d *spaces.Decision) { d.ImportSourceType = "notion" }, nil), "decision_invalid"},
		"no source type":       {signFileGrantToken(t, func(d *spaces.Decision) { d.ImportSourceType = "" }, nil), "decision_invalid"},
		"zero data retention":  {signFileGrantToken(t, func(d *spaces.Decision) { d.ZeroDataRetention = true }, nil), string(spaces.DenialZDRForbidsEffect)},
	}
	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			rec := authzDo(engine, http.MethodPost, fileGrantPath, fileGrantBody(tc.token))
			if rec.Code != http.StatusForbidden {
				t.Fatalf("status = %d, want 403 (%s)", rec.Code, rec.Body.String())
			}
			var decoded map[string]any
			_ = json.Unmarshal(rec.Body.Bytes(), &decoded)
			if decoded["code"] != tc.code {
				t.Fatalf("code = %v, want %s (%s)", decoded["code"], tc.code, rec.Body.String())
			}
			if strings.Contains(rec.Body.String(), "grant_id") {
				t.Fatalf("a refusal must not carry a grant: %s", rec.Body.String())
			}
		})
	}
}

func TestFileGrantAndSpaceVisibleRoutesAreOnTheEngineOnce(t *testing.T) {
	gin.SetMode(gin.TestMode)
	counts := routeCounts(NewServer(nil, nil, nil, nil, "0").router.Routes())
	for _, route := range []string{
		"POST " + fileGrantPath,
		"GET " + authzSpaceVisiblePath,
		"POST " + authzGrantPath,
		"DELETE " + authzGrantPath,
		"GET " + authzGrantsPath,
	} {
		if counts[route] != 1 {
			t.Errorf("%s registered %d times, want 1", route, counts[route])
		}
	}
}
