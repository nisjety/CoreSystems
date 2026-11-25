package api

import (
	"bytes"
	"crypto/ed25519"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

// F-f10-19: the Space skill write decision verifier and the skill routes that use it.

const (
	spaceSkillTestKeyID   = "control-test-key"
	spaceSkillTestSpace   = "room-1"
	spaceSkillCreateBody  = `{"name":"room skill","content":"body","scope":"space","scope_id":"room-1"}`
	spaceSkillPatchBody   = `{"content":"edited"}`
	spaceSkillTestSkillID = "sk-1"
)

var (
	spaceSkillTestPrivate = ed25519.NewKeyFromSeed(bytes.Repeat([]byte{0x11}, ed25519.SeedSize))
	spaceSkillOtherKey    = ed25519.NewKeyFromSeed(bytes.Repeat([]byte{0x22}, ed25519.SeedSize))
)

func newTestSpaceSkillVerifier(t *testing.T) *SpaceSkillDecisionVerifier {
	t.Helper()
	public := spaceSkillTestPrivate.Public().(ed25519.PublicKey)
	verifier, err := NewSpaceSkillDecisionVerifier(spaceSkillTestKeyID, base64.RawURLEncoding.EncodeToString(public))
	if err != nil {
		t.Fatal(err)
	}
	return verifier
}

// spaceSkillTestDecision is a decision user-core would issue for want, as a JSON object a test can
// change field by field before it is signed.
func spaceSkillTestDecision(t *testing.T, want SkillWriteIntent, now time.Time) map[string]any {
	t.Helper()
	decision := spaceSkillDecision{
		DecisionRef: "decision-1", OrgID: want.OrgID, SpaceRef: want.SpaceRef, SubjectID: want.SubjectID,
		ServiceAudience: spaceSkillWriteAudience, ActionID: spaceSkillWriteAction, ActionSchemaHash: spaceSkillWriteSchema,
		IdempotencyKey: "idem-1", RecipientAudienceRef: "audience-1", RecipientAudienceHash: "sha256:audience",
		PrivacyPolicyRef: "privacy-1", ResourceAuthorizationRef: "control:" + want.SpaceRef + ":skill-write:7",
		AuthorityRevision: 7, RecipientAudienceRevision: 3, Permissions: []string{spaceSkillWritePermission},
		IssuedAt: now.UTC(), ExpiresAt: now.UTC().Add(spaceSkillDecisionLifetime), Nonce: "nonce-" + want.Operation + "-" + now.Format(time.RFC3339Nano),
	}
	decision.PayloadDigest = spaceSkillWritePayloadDigest(decision, want)
	raw, err := json.Marshal(decision)
	if err != nil {
		t.Fatal(err)
	}
	var fields map[string]any
	if err := json.Unmarshal(raw, &fields); err != nil {
		t.Fatal(err)
	}
	// Fields user-core also signs that this verifier does not read.
	fields["membership_revision"] = 2
	fields["purpose"] = "collaboration"
	return fields
}

func signSpaceSkillDecision(t *testing.T, key ed25519.PrivateKey, keyID string, fields map[string]any) string {
	t.Helper()
	payload, err := json.Marshal(fields)
	if err != nil {
		t.Fatal(err)
	}
	signed := "v2." + base64.RawURLEncoding.EncodeToString([]byte(keyID)) + "." + base64.RawURLEncoding.EncodeToString(payload)
	return signed + "." + base64.RawURLEncoding.EncodeToString(ed25519.Sign(key, []byte(signed)))
}

func spaceSkillTestIntent(t *testing.T, operation, skillID, body string) SkillWriteIntent {
	t.Helper()
	digest, err := spaceSkillRequestDigest(operation, []byte(body))
	if err != nil {
		t.Fatal(err)
	}
	return SkillWriteIntent{OrgID: "org-a", SpaceRef: spaceSkillTestSpace, SubjectID: "user-a",
		Operation: operation, SkillID: skillID, RequestDigest: digest}
}

func validSpaceSkillToken(t *testing.T, want SkillWriteIntent) string {
	t.Helper()
	return signSpaceSkillDecision(t, spaceSkillTestPrivate, spaceSkillTestKeyID, spaceSkillTestDecision(t, want, time.Now()))
}

func TestSkillSpaceWriteVerifyReadsOnlyTheLiteralTrue(t *testing.T) {
	for value, want := range map[string]bool{"true": true, "": false, "1": false, "TRUE": false, "on": false, " true": false} {
		if got := SkillSpaceWriteVerifyFromLookup(func(string) string { return value }); got != want {
			t.Errorf("SKILL_SPACE_WRITE_VERIFY=%q read as %v, want %v", value, got, want)
		}
	}
}

// The payload digest is pinned to a vector computed by a verbatim copy of user-core's
// spaceSkillWritePayloadDigest (F-f10-18), so a drift on either side fails here.
func TestSpaceSkillPayloadDigestMatchesControl(t *testing.T) {
	decision := spaceSkillDecision{
		DecisionRef: "decision-1", OrgID: "org-a", SpaceRef: "room-1", SubjectID: "user-a",
		RecipientAudienceRef: "audience-1", RecipientAudienceHash: "sha256:audience", PrivacyPolicyRef: "privacy-1",
		ResourceAuthorizationRef: "control:room-1:skill-write:7", IdempotencyKey: "idem-1",
		AuthorityRevision: 7, RecipientAudienceRevision: 3,
	}
	want := SkillWriteIntent{Operation: "update", SkillID: "sk-1",
		RequestDigest: "sha256:ab0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcd"}
	const control = "sha256:fca098ecd6623c581113b4c39a357132cc10b6e3c1efd7028e77ffea7f50df4f"
	if got := spaceSkillWritePayloadDigest(decision, want); got != control {
		t.Fatalf("payload digest = %s, want Control's %s", got, control)
	}
}

func TestSpaceSkillRequestDigestIsOfTheCanonicalBody(t *testing.T) {
	a, err := spaceSkillRequestDigest(spaceSkillOperationUpdate, []byte(`{"b":1.50,"a":"<x>"}`))
	if err != nil {
		t.Fatal(err)
	}
	b, err := spaceSkillRequestDigest(spaceSkillOperationUpdate, []byte(" {\n \"a\" : \"<x>\", \"b\": 1.50 }\n"))
	if err != nil {
		t.Fatal(err)
	}
	if a != b {
		t.Fatalf("equivalent bodies digest differently: %s vs %s", a, b)
	}
	// sha256 of `{"a":"<x>","b":1.50}`: keys sorted, no HTML escaping, the number as sent.
	if a != "sha256:"+hexSHA256(`{"a":"<x>","b":1.50}`) {
		t.Fatalf("digest %s is not of the canonical form", a)
	}
	c, _ := spaceSkillRequestDigest(spaceSkillOperationUpdate, []byte(`{"a":"<y>","b":1.50}`))
	if c == a {
		t.Fatal("a different body must digest differently")
	}
	empty, err := spaceSkillRequestDigest(spaceSkillOperationDelete, []byte(`{"ignored":true}`))
	if err != nil || empty != "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855" {
		t.Fatalf("a delete digests the empty body, got %s (%v)", empty, err)
	}
	if _, err := spaceSkillRequestDigest(spaceSkillOperationCreate, []byte(`not json`)); err == nil {
		t.Fatal("a body that is not JSON has no digest")
	}
}

func hexSHA256(value string) string {
	sum := sha256.Sum256([]byte(value))
	return hex.EncodeToString(sum[:])
}

func TestSpaceSkillVerifierAcceptsOnlyTheExactWrite(t *testing.T) {
	want := spaceSkillTestIntent(t, spaceSkillOperationUpdate, spaceSkillTestSkillID, spaceSkillPatchBody)
	if err := newTestSpaceSkillVerifier(t).Verify(validSpaceSkillToken(t, want), want); err != nil {
		t.Fatalf("a valid decision was refused: %v", err)
	}

	other := func(change func(*SkillWriteIntent)) SkillWriteIntent {
		changed := want
		change(&changed)
		return changed
	}
	for name, presented := range map[string]SkillWriteIntent{
		"other org":       other(func(i *SkillWriteIntent) { i.OrgID = "org-b" }),
		"other Space":     other(func(i *SkillWriteIntent) { i.SpaceRef = "room-2" }),
		"other person":    other(func(i *SkillWriteIntent) { i.SubjectID = "user-b" }),
		"other operation": other(func(i *SkillWriteIntent) { i.Operation = spaceSkillOperationDelete }),
		"other skill":     other(func(i *SkillWriteIntent) { i.SkillID = "sk-2" }),
		"other body":      spaceSkillTestIntent(t, spaceSkillOperationUpdate, spaceSkillTestSkillID, `{"content":"something else"}`),
	} {
		t.Run(name, func(t *testing.T) {
			if err := newTestSpaceSkillVerifier(t).Verify(validSpaceSkillToken(t, want), presented); !errors.Is(err, errSpaceSkillDecisionInvalid) {
				t.Fatalf("err = %v, want invalid", err)
			}
		})
	}

	now := time.Now()
	for name, mutate := range map[string]func(map[string]any){
		"audience":         func(f map[string]any) { f["service_audience"] = "model-plane-session-core" },
		"action":           func(f map[string]any) { f["action_id"] = "model.space.memory.manage" },
		"schema":           func(f map[string]any) { f["action_schema_hash"] = "sha256:space-skill-write-v2" },
		"extra permission": func(f map[string]any) { f["permissions"] = []string{"skill:write", "memory:manage"} },
		"other permission": func(f map[string]any) { f["permissions"] = []string{"memory:manage"} },
		"no permission":    func(f map[string]any) { f["permissions"] = []string{} },
		"org":              func(f map[string]any) { f["org_id"] = "org-b" },
		"space":            func(f map[string]any) { f["space_ref"] = "room-2" },
		"subject":          func(f map[string]any) { f["subject_id"] = "user-b" },
		"payload digest":   func(f map[string]any) { f["payload_digest"] = "sha256:" + strings.Repeat("0", 64) },
		"revision":         func(f map[string]any) { f["authority_revision"] = 8 },
		"expired": func(f map[string]any) {
			f["issued_at"] = now.Add(-2 * time.Minute)
			f["expires_at"] = now.Add(-time.Minute)
		},
		"issued in future": func(f map[string]any) {
			f["issued_at"] = now.Add(10 * time.Second)
			f["expires_at"] = now.Add(50 * time.Second)
		},
		"lifetime too long":  func(f map[string]any) { f["expires_at"] = now.Add(10 * time.Minute) },
		"no nonce":           func(f map[string]any) { f["nonce"] = "" },
		"no decision ref":    func(f map[string]any) { f["decision_ref"] = "" },
		"no idempotency key": func(f map[string]any) { f["idempotency_key"] = "" },
	} {
		t.Run("decision "+name, func(t *testing.T) {
			fields := spaceSkillTestDecision(t, want, now)
			mutate(fields)
			token := signSpaceSkillDecision(t, spaceSkillTestPrivate, spaceSkillTestKeyID, fields)
			if err := newTestSpaceSkillVerifier(t).Verify(token, want); !errors.Is(err, errSpaceSkillDecisionInvalid) {
				t.Fatalf("err = %v, want invalid", err)
			}
		})
	}

	fields := spaceSkillTestDecision(t, want, now)
	for name, token := range map[string]string{
		"other key":      signSpaceSkillDecision(t, spaceSkillOtherKey, spaceSkillTestKeyID, fields),
		"other key id":   signSpaceSkillDecision(t, spaceSkillTestPrivate, "other-key", fields),
		"empty":          "",
		"not a decision": "v2.a.b",
		"version":        strings.Replace(signSpaceSkillDecision(t, spaceSkillTestPrivate, spaceSkillTestKeyID, fields), "v2.", "v1.", 1),
	} {
		t.Run("token "+name, func(t *testing.T) {
			if err := newTestSpaceSkillVerifier(t).Verify(token, want); !errors.Is(err, errSpaceSkillDecisionInvalid) {
				t.Fatalf("err = %v, want invalid", err)
			}
		})
	}
}

func TestSpaceSkillVerifierRefusesAReplayAndAZDRSpace(t *testing.T) {
	want := spaceSkillTestIntent(t, spaceSkillOperationDelete, spaceSkillTestSkillID, "")
	verifier := newTestSpaceSkillVerifier(t)
	token := validSpaceSkillToken(t, want)
	if err := verifier.Verify(token, want); err != nil {
		t.Fatalf("first use refused: %v", err)
	}
	if err := verifier.Verify(token, want); !errors.Is(err, errSpaceSkillDecisionReplayed) {
		t.Fatalf("replay err = %v, want replayed", err)
	}

	fields := spaceSkillTestDecision(t, want, time.Now())
	fields["zero_data_retention"] = true
	fields["nonce"] = "nonce-zdr"
	if err := verifier.Verify(signSpaceSkillDecision(t, spaceSkillTestPrivate, spaceSkillTestKeyID, fields), want); !errors.Is(err, errSpaceSkillDecisionZDR) {
		t.Fatalf("ZDR err = %v, want the ZDR refusal", err)
	}
}

func TestSpaceSkillNonceCacheIsBoundedAndForgetsExpiredNonces(t *testing.T) {
	verifier := newTestSpaceSkillVerifier(t)
	now := time.Now()
	if !verifier.useNonce("old", now.Add(time.Second), now) {
		t.Fatal("a fresh nonce must be accepted")
	}
	if !verifier.useNonce("old", now.Add(time.Minute), now.Add(2*time.Second)) {
		t.Fatal("an expired nonce is forgotten (its decision can no longer verify)")
	}
	for i := 0; i < spaceSkillNonceCapacity+10; i++ {
		verifier.useNonce("n"+strings.Repeat("x", i%7)+time.Duration(i).String(), now.Add(time.Minute), now)
	}
	if verifier.order.Len() > spaceSkillNonceCapacity || len(verifier.nonces) != verifier.order.Len() {
		t.Fatalf("cache holds %d/%d entries, capacity %d", verifier.order.Len(), len(verifier.nonces), spaceSkillNonceCapacity)
	}
}

// spaceSkillRoutes is the skills routes with SKILL_SPACE_SCOPE on, SKILL_SPACE_WRITE_VERIFY as given
// and the test verifier wired, behind the test authenticator. The same handler serves every request,
// so a replay reaches the same nonce cache.
type spaceSkillRoutes struct {
	handler http.Handler
	sign    mcpTokenSigner
}

func newSpaceSkillRoutes(t *testing.T, pool registryDatabase, verify bool) spaceSkillRoutes {
	t.Helper()
	t.Setenv(skillSpaceScopeEnv, "true")
	if verify {
		t.Setenv(skillSpaceWriteVerifyEnv, "true")
	} else {
		t.Setenv(skillSpaceWriteVerifyEnv, "")
	}
	skills := NewSkillsHandler(pool).WithSpaceSkillDecisionVerifier(newTestSpaceSkillVerifier(t))
	mux := http.NewServeMux()
	skills.Register(mux)
	handler, sign := mcpAuthenticatedHandlerWithSigner(t, mux)
	return spaceSkillRoutes{handler: handler, sign: sign}
}

func (routes spaceSkillRoutes) serve(token func(mcpTokenSigner) string, method, target, body, decision string) *httptest.ResponseRecorder {
	request := httptest.NewRequest(method, target, strings.NewReader(body))
	request.Header.Set("Authorization", "Bearer "+token(routes.sign))
	if decision != "" {
		request.Header.Set(spaceSkillDecisionHeader, decision)
	}
	response := httptest.NewRecorder()
	routes.handler.ServeHTTP(response, request)
	return response
}

func requireTypedRefusal(t *testing.T, response *httptest.ResponseRecorder, status int, code string) {
	t.Helper()
	if response.Code != status {
		t.Fatalf("status = %d, body=%s, want %d", response.Code, response.Body.String(), status)
	}
	var envelope struct {
		Error struct {
			Code string `json:"code"`
		} `json:"error"`
	}
	if err := json.Unmarshal(response.Body.Bytes(), &envelope); err != nil || envelope.Error.Code != code {
		t.Fatalf("refusal = %s, want code %q", response.Body.String(), code)
	}
}

// spaceSkillStoredDatabase holds one stored Space skill of room-1: the ownership read under the row
// lock answers scope 'space', the Space read answers its scope_id.
func spaceSkillStoredDatabase() *recordingDatabase {
	database := ownedSkillDatabase(spaceSkillScope, "")
	database.nextRows = append(database.nextRows, stringRow{value: spaceSkillTestSpace})
	return database
}

func TestSpaceSkillWriteWithAVerifiedDecision(t *testing.T) {
	t.Run("create", func(t *testing.T) {
		database := &recordingDatabase{}
		routes := newSpaceSkillRoutes(t, database, true)
		token := validSpaceSkillToken(t, spaceSkillTestIntent(t, spaceSkillOperationCreate, "", spaceSkillCreateBody))
		response := routes.serve(writeToken, http.MethodPost, "/api/v1/skills", spaceSkillCreateBody, token)
		if response.Code != http.StatusCreated {
			t.Fatalf("status = %d, body=%s, want 201", response.Code, response.Body.String())
		}
		if len(database.execs) != 1 || database.execs[0].query != spaceSkillInsertSQL || database.commits != 1 {
			t.Fatalf("want one Space insert, committed: execs=%#v commits=%d", database.execs, database.commits)
		}
		args := database.execs[0].args
		if args[9] != spaceSkillScope || args[10] != "" || args[14] != spaceSkillTestSpace {
			t.Fatalf("insert args = %#v, want scope space, no owner, scope_id room-1", args)
		}
	})
	for _, test := range []struct {
		name, method, body, operation string
		want                          int
	}{
		{"patch", http.MethodPatch, spaceSkillPatchBody, spaceSkillOperationUpdate, http.StatusOK},
		{"delete", http.MethodDelete, "", spaceSkillOperationDelete, http.StatusNoContent},
	} {
		t.Run(test.name, func(t *testing.T) {
			database := spaceSkillStoredDatabase()
			routes := newSpaceSkillRoutes(t, database, true)
			token := validSpaceSkillToken(t, spaceSkillTestIntent(t, test.operation, spaceSkillTestSkillID, test.body))
			response := routes.serve(writeToken, test.method, "/api/v1/skills/"+spaceSkillTestSkillID, test.body, token)
			if response.Code != test.want {
				t.Fatalf("status = %d, body=%s, want %d", response.Code, response.Body.String(), test.want)
			}
			if len(database.queryRows) != 1 || database.queryRows[0].query != storedSpaceSkillRefSQL {
				t.Fatalf("the Space must be read from the stored row: %#v", database.queryRows)
			}
			if len(database.execs) != 1 || database.commits != 1 {
				t.Fatalf("want one write, committed: execs=%#v commits=%d", database.execs, database.commits)
			}
		})
	}
}

func TestSpaceSkillWriteRefusesWithoutAMatchingDecision(t *testing.T) {
	createIntent := spaceSkillTestIntent(t, spaceSkillOperationCreate, "", spaceSkillCreateBody)
	patchIntent := spaceSkillTestIntent(t, spaceSkillOperationUpdate, spaceSkillTestSkillID, spaceSkillPatchBody)
	deleteIntent := spaceSkillTestIntent(t, spaceSkillOperationDelete, spaceSkillTestSkillID, "")
	expired := func(want SkillWriteIntent) string {
		fields := spaceSkillTestDecision(t, want, time.Now().Add(-2*time.Minute))
		return signSpaceSkillDecision(t, spaceSkillTestPrivate, spaceSkillTestKeyID, fields)
	}
	otherSpace := func(want SkillWriteIntent) string {
		want.SpaceRef = "room-2"
		return validSpaceSkillToken(t, want)
	}
	type refusal struct {
		name, method, target, body, decision, code string
		status                                     int
	}
	cases := []refusal{
		{"create without the header", http.MethodPost, "/api/v1/skills", spaceSkillCreateBody, "", spaceSkillDecisionRequiredCode, http.StatusForbidden},
		{"create for another Space", http.MethodPost, "/api/v1/skills", spaceSkillCreateBody, otherSpace(createIntent), spaceSkillDecisionInvalidCode, http.StatusForbidden},
		{"create with another body", http.MethodPost, "/api/v1/skills", strings.Replace(spaceSkillCreateBody, `"body"`, `"other"`, 1), validSpaceSkillToken(t, createIntent), spaceSkillDecisionInvalidCode, http.StatusForbidden},
		{"create with an update decision", http.MethodPost, "/api/v1/skills", spaceSkillCreateBody, validSpaceSkillToken(t, spaceSkillTestIntent(t, spaceSkillOperationUpdate, "sk-x", spaceSkillCreateBody)), spaceSkillDecisionInvalidCode, http.StatusForbidden},
		{"create with an expired decision", http.MethodPost, "/api/v1/skills", spaceSkillCreateBody, expired(createIntent), spaceSkillDecisionInvalidCode, http.StatusForbidden},
		{"patch without the header", http.MethodPatch, "/api/v1/skills/sk-1", spaceSkillPatchBody, "", spaceSkillDecisionRequiredCode, http.StatusForbidden},
		{"patch of another skill", http.MethodPatch, "/api/v1/skills/sk-1", spaceSkillPatchBody, validSpaceSkillToken(t, spaceSkillTestIntent(t, spaceSkillOperationUpdate, "sk-2", spaceSkillPatchBody)), spaceSkillDecisionInvalidCode, http.StatusForbidden},
		{"patch with another body", http.MethodPatch, "/api/v1/skills/sk-1", `{"content":"other"}`, validSpaceSkillToken(t, patchIntent), spaceSkillDecisionInvalidCode, http.StatusForbidden},
		{"patch for another Space", http.MethodPatch, "/api/v1/skills/sk-1", spaceSkillPatchBody, otherSpace(patchIntent), spaceSkillDecisionInvalidCode, http.StatusForbidden},
		{"patch with a delete decision", http.MethodPatch, "/api/v1/skills/sk-1", spaceSkillPatchBody, validSpaceSkillToken(t, spaceSkillTestIntent(t, spaceSkillOperationDelete, spaceSkillTestSkillID, spaceSkillPatchBody)), spaceSkillDecisionInvalidCode, http.StatusForbidden},
		{"patch with an expired decision", http.MethodPatch, "/api/v1/skills/sk-1", spaceSkillPatchBody, expired(patchIntent), spaceSkillDecisionInvalidCode, http.StatusForbidden},
		{"delete without the header", http.MethodDelete, "/api/v1/skills/sk-1", "", "", spaceSkillDecisionRequiredCode, http.StatusForbidden},
		{"delete with an update decision", http.MethodDelete, "/api/v1/skills/sk-1", "", validSpaceSkillToken(t, spaceSkillTestIntent(t, spaceSkillOperationUpdate, spaceSkillTestSkillID, "{}")), spaceSkillDecisionInvalidCode, http.StatusForbidden},
		{"delete for another Space", http.MethodDelete, "/api/v1/skills/sk-1", "", otherSpace(deleteIntent), spaceSkillDecisionInvalidCode, http.StatusForbidden},
		{"delete with an expired decision", http.MethodDelete, "/api/v1/skills/sk-1", "", expired(deleteIntent), spaceSkillDecisionInvalidCode, http.StatusForbidden},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			database := spaceSkillStoredDatabase()
			routes := newSpaceSkillRoutes(t, database, true)
			response := routes.serve(writeToken, test.method, test.target, test.body, test.decision)
			requireTypedRefusal(t, response, test.status, test.code)
			if len(database.execs) != 0 || database.commits != 0 {
				t.Fatalf("a refused write must write and commit nothing: execs=%#v commits=%d", database.execs, database.commits)
			}
		})
	}

	t.Run("a service principal never authors", func(t *testing.T) {
		database := &recordingDatabase{}
		routes := newSpaceSkillRoutes(t, database, true)
		serviceToken := func(sign mcpTokenSigner) string {
			nonZDR := false
			return sign("service", []string{"capability:write", "admin"}, &nonZDR)
		}
		response := routes.serve(serviceToken, http.MethodPost, "/api/v1/skills", spaceSkillCreateBody, validSpaceSkillToken(t, createIntent))
		if response.Code == http.StatusCreated || len(database.execs) != 0 {
			t.Fatalf("a service must not write a Space skill: status=%d execs=%#v", response.Code, database.execs)
		}
	})

	t.Run("a replay", func(t *testing.T) {
		database := spaceSkillStoredDatabase()
		routes := newSpaceSkillRoutes(t, database, true)
		token := validSpaceSkillToken(t, deleteIntent)
		if first := routes.serve(writeToken, http.MethodDelete, "/api/v1/skills/sk-1", "", token); first.Code != http.StatusNoContent {
			t.Fatalf("first use: status = %d, body=%s", first.Code, first.Body.String())
		}
		database.nextRows = append(database.nextRows, stringRow{value: spaceSkillTestSpace})
		database.rows = &skillRows{rows: [][]any{{spaceSkillScope, ""}}}
		replay := routes.serve(writeToken, http.MethodDelete, "/api/v1/skills/sk-1", "", token)
		requireTypedRefusal(t, replay, http.StatusConflict, spaceSkillDecisionReplayedCode)
		if len(database.execs) != 1 || database.commits != 1 {
			t.Fatalf("the replay must write nothing: execs=%#v commits=%d", database.execs, database.commits)
		}
	})

	t.Run("a ZDR Space", func(t *testing.T) {
		database := &recordingDatabase{}
		routes := newSpaceSkillRoutes(t, database, true)
		fields := spaceSkillTestDecision(t, createIntent, time.Now())
		fields["zero_data_retention"] = true
		response := routes.serve(writeToken, http.MethodPost, "/api/v1/skills", spaceSkillCreateBody,
			signSpaceSkillDecision(t, spaceSkillTestPrivate, spaceSkillTestKeyID, fields))
		requireTypedRefusal(t, response, http.StatusPreconditionFailed, "zdr_feature_not_supported")
		if database.databaseCalls() != 0 {
			t.Fatalf("a ZDR refusal must not reach the database: %d calls", database.databaseCalls())
		}
	})
}

// With SKILL_SPACE_WRITE_VERIFY off, even a valid decision is not read: every Space skill write is
// refused exactly as before, with space_skill_decision_required and nothing written.
func TestSpaceSkillWriteSwitchOffKeepsTodaysRefusal(t *testing.T) {
	for _, test := range []struct {
		name, method, target, body string
		want                       SkillWriteIntent
	}{
		{"create", http.MethodPost, "/api/v1/skills", spaceSkillCreateBody, spaceSkillTestIntent(t, spaceSkillOperationCreate, "", spaceSkillCreateBody)},
		{"patch", http.MethodPatch, "/api/v1/skills/sk-1", spaceSkillPatchBody, spaceSkillTestIntent(t, spaceSkillOperationUpdate, spaceSkillTestSkillID, spaceSkillPatchBody)},
		{"delete", http.MethodDelete, "/api/v1/skills/sk-1", "", spaceSkillTestIntent(t, spaceSkillOperationDelete, spaceSkillTestSkillID, "")},
	} {
		t.Run(test.name, func(t *testing.T) {
			database := spaceSkillStoredDatabase()
			routes := newSpaceSkillRoutes(t, database, false)
			response := routes.serve(adminToken, test.method, test.target, test.body, validSpaceSkillToken(t, test.want))
			requireSpaceDecisionRefusal(t, response.Code, response.Body.Bytes())
			if len(database.execs) != 0 || database.commits != 0 || len(database.queryRows) != 0 {
				t.Fatalf("off, nothing is read or written past the refusal: execs=%#v queryRows=%#v commits=%d",
					database.execs, database.queryRows, database.commits)
			}
		})
	}
}
