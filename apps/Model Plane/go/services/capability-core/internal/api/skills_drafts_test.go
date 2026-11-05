package api

import (
	"encoding/json"
	"errors"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/triodelab/model-plane/services/capability-core/internal/skillbundle"
)

// EP-11 S5: draft, publish, discard and restore on the skills registry.

// bytesRow answers a one-column read of a JSON body.
type bytesRow struct{ value []byte }

func (row bytesRow) Scan(dest ...any) error {
	if len(dest) != 1 {
		return errors.New("bytesRow: want one scan destination")
	}
	*(dest[0].(*[]byte)) = row.value
	return nil
}

// publishedRow answers publish's RETURNING version, bundle_sha256.
type publishedRow struct {
	version int32
	bundle  string
}

func (row publishedRow) Scan(dest ...any) error {
	if len(dest) != 2 {
		return errors.New("publishedRow: want two scan destinations")
	}
	*(dest[0].(*int32)) = row.version
	*(dest[1].(*string)) = row.bundle
	return nil
}

// versionBodyRow answers restore's read of a version: its body and bundle digest.
type versionBodyRow struct {
	body   []byte
	bundle string
}

func (row versionBodyRow) Scan(dest ...any) error {
	if len(dest) != 2 {
		return errors.New("versionBodyRow: want two scan destinations")
	}
	*(dest[0].(*[]byte)) = row.body
	*(dest[1].(*string)) = row.bundle
	return nil
}

func withDrafts(t *testing.T, on bool) {
	t.Helper()
	value := ""
	if on {
		value = "true"
	}
	t.Setenv(draftPublishEnv, value)
	t.Setenv(skillbundle.RetentionEnv, "")
}

func TestDraftPublishReadsOnlyTheLiteralTrue(t *testing.T) {
	for value, want := range map[string]bool{"true": true, "": false, "1": false, "TRUE": false, "on": false} {
		if got := DraftPublishFromLookup(func(string) string { return value }); got != want {
			t.Errorf("SKILL_DRAFT_PUBLISH=%q read as %v, want %v", value, got, want)
		}
	}
}

// Off, the draft paths fall to the skill subtree as before and nothing is read or written.
func TestDraftRoutesAreAbsentWhileTheSwitchIsOff(t *testing.T) {
	withDrafts(t, false)
	for _, test := range []struct{ method, path string }{
		{http.MethodPost, "/api/v1/skills/sk-1/publish"},
		{http.MethodPost, "/api/v1/skills/sk-1/versions/1/restore"},
	} {
		database := &recordingDatabase{}
		response := serveSkill(t, database, writeToken, test.method, test.path, "")
		if response.Code != http.StatusNotFound {
			t.Errorf("%s %s: status = %d, want 404", test.method, test.path, response.Code)
		}
		if database.databaseCalls() != 0 || database.commits != 0 {
			t.Errorf("%s %s: the registry was touched with the switch off", test.method, test.path)
		}
	}
}

// Off, a PATCH of an accepted skill's text edits the live row as before and never asks whether
// the skill is accepted.
func TestPatchEditsLiveWhileTheSwitchIsOff(t *testing.T) {
	withDrafts(t, false)
	database := &recordingDatabase{}
	response := serveSkill(t, database, writeToken, http.MethodPatch, "/api/v1/skills/sk-1", `{"content":"edited"}`)
	if response.Code != http.StatusOK {
		t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
	}
	if len(database.queryRows) != 0 || len(database.execs) != 1 || database.execs[0].query == draftPatchSQL {
		t.Fatalf("the PATCH must be today's live update: %#v %#v", database.queryRows, database.execs)
	}
	if strings.Contains(response.Body.String(), `"draft"`) {
		t.Fatalf("an off body must be as it was: %s", response.Body.String())
	}
}

func TestPatchWritesDraftWhenOn(t *testing.T) {
	withDrafts(t, true)
	database := &recordingDatabase{}
	database.nextRows = append(database.nextRows, boolRow{value: true})
	response := serveSkill(t, database, writeToken, http.MethodPatch, "/api/v1/skills/sk-1", `{"content":"edited"}`)
	if response.Code != http.StatusOK {
		t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
	}
	if len(database.execs) != 1 || database.execs[0].query != draftPatchSQL {
		t.Fatalf("an accepted skill's edit must write its draft: %#v", database.execs)
	}
	args := database.execs[0].args
	if args[2] != "edited" || args[5] != "sk-1" || args[6] != "org-a" || args[8] != "user-a" {
		t.Fatalf("draft args = %#v", args)
	}
	var body map[string]any
	if err := json.Unmarshal(response.Body.Bytes(), &body); err != nil || body["draft"] != true {
		t.Fatalf("body = %s, want draft true", response.Body.String())
	}
	if strings.Contains(draftPatchSQL, "content=COALESCE") || strings.Contains(draftPatchSQL, "description=COALESCE") {
		t.Fatal("a draft write must not touch the live text")
	}
}

// A proposal has no draft: its owner edits it live, and accepting it is its publish.
func TestPatchOfAProposalStaysLiveWhenOn(t *testing.T) {
	withDrafts(t, true)
	database := &recordingDatabase{}
	database.nextRows = append(database.nextRows, boolRow{value: false})
	response := serveSkill(t, database, writeToken, http.MethodPatch, "/api/v1/skills/sk-1", `{"content":"edited"}`)
	if response.Code != http.StatusOK {
		t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
	}
	if len(database.execs) != 1 || database.execs[0].query == draftPatchSQL {
		t.Fatalf("a proposal's edit must stay live: %#v", database.execs)
	}
}

// Switching an accepted skill off or sharing it writes no text, so it needs no draft.
func TestPatchWithoutTextNeedsNoDraft(t *testing.T) {
	withDrafts(t, true)
	database := &recordingDatabase{}
	response := serveSkill(t, database, writeToken, http.MethodPatch, "/api/v1/skills/sk-1", `{"enabled":false}`)
	if response.Code != http.StatusOK || len(database.queryRows) != 0 || database.execs[0].query == draftPatchSQL {
		t.Fatalf("status = %d, reads = %#v, execs = %#v", response.Code, database.queryRows, database.execs)
	}
}

func TestPublishCutsOneVersion(t *testing.T) {
	withDrafts(t, true)
	database := &recordingDatabase{}
	draft := []byte(`{"name":"deploy","description":"d","content":"new steps","trigger_keywords":[],"tool_restrictions":[],"kind":"skill","settings":{},"bundle_sha256":""}`)
	database.nextRows = append(database.nextRows, boolRow{value: true}, bytesRow{value: draft}, publishedRow{version: 3})
	response := serveSkill(t, database, writeToken, http.MethodPost, "/api/v1/skills/sk-1/publish", "")
	if response.Code != http.StatusOK {
		t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
	}
	publishes := 0
	for _, call := range database.queryRows {
		if call.query == publishSkillSQL(false) {
			publishes++
		}
	}
	if publishes != 1 || database.commits != 1 {
		t.Fatalf("publish statements = %d, commits = %d, want one of each", publishes, database.commits)
	}
	if len(database.execs) != 0 {
		t.Fatalf("with retention off a publish touches no bundle: %#v", database.execs)
	}
	if args := database.actorCalls[0].args; args[1] != "user-a" || args[2] != "draft_published" || args[3] != "" {
		t.Fatalf("actor args = %#v, want the publisher, a reason and no decision (the revision reads edited)", args)
	}
	var body struct {
		Version   int32 `json:"version"`
		Published bool  `json:"published"`
	}
	if err := json.Unmarshal(response.Body.Bytes(), &body); err != nil || body.Version != 3 || !body.Published {
		t.Fatalf("body = %s", response.Body.String())
	}
	if strings.Contains(publishSkillSQL(false), "bundle_sha256 = COALESCE") || !strings.Contains(publishSkillSQL(true), "bundle_sha256 = COALESCE") {
		t.Fatal("only a publish with retention on may move the bundle digest")
	}
	if strings.Contains(publishSkillSQL(true), "kind =") {
		t.Fatal("a publish never changes the kind")
	}
}

func TestPublishWithoutADraftIsNotFound(t *testing.T) {
	withDrafts(t, true)
	database := &recordingDatabase{}
	database.nextRows = append(database.nextRows, boolRow{value: true}, bytesRow{value: nil})
	response := serveSkill(t, database, writeToken, http.MethodPost, "/api/v1/skills/sk-1/publish", "")
	if response.Code != http.StatusNotFound || typedErrorCode(t, response.Body.Bytes()) != "skill_draft_not_found" {
		t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
	}
	if database.commits != 0 {
		t.Fatal("nothing to publish must commit nothing")
	}
}

func TestPublishRequiresOwnerOrAdmin(t *testing.T) {
	withDrafts(t, true)
	database := ownedSkillDatabase("user", "user-b")
	response := serveSkill(t, database, writeToken, http.MethodPost, "/api/v1/skills/sk-1/publish", "")
	if response.Code != http.StatusForbidden || typedErrorCode(t, response.Body.Bytes()) != "forbidden" {
		t.Fatalf("status = %d, body=%s, want 403", response.Code, response.Body.String())
	}
	if len(database.queryRows) != 0 || len(database.execs) != 0 || database.commits != 0 {
		t.Fatal("a refused publish must read no draft and write nothing")
	}
	admin := ownedSkillDatabase("user", "user-b")
	admin.nextRows = append(admin.nextRows, boolRow{value: true}, bytesRow{value: []byte(`{"content":"x"}`)}, publishedRow{version: 2})
	if response := serveSkill(t, admin, adminToken, http.MethodPost, "/api/v1/skills/sk-1/publish", ""); response.Code != http.StatusOK {
		t.Fatalf("an admin publish: status = %d, body=%s", response.Code, response.Body.String())
	}
}

// A proposal is published by its decision, never by this route.
func TestPublishRefusesAProposal(t *testing.T) {
	withDrafts(t, true)
	database := &recordingDatabase{}
	database.nextRows = append(database.nextRows, boolRow{value: false})
	response := serveSkill(t, database, writeToken, http.MethodPost, "/api/v1/skills/sk-1/publish", "")
	if response.Code != http.StatusConflict || typedErrorCode(t, response.Body.Bytes()) != "skill_not_published" {
		t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
	}
}

func TestRestoreWritesDraftNotLive(t *testing.T) {
	withDrafts(t, true)
	database := &recordingDatabase{}
	body := []byte(`{"name":"deploy","content":"old steps","bundle_sha256":""}`)
	database.nextRows = append(database.nextRows, boolRow{value: true}, versionBodyRow{body: body})
	response := serveSkill(t, database, writeToken, http.MethodPost, "/api/v1/skills/sk-1/versions/1/restore", "")
	if response.Code != http.StatusOK {
		t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
	}
	if len(database.execs) != 1 || database.execs[0].query != restoreDraftSQL {
		t.Fatalf("a restore must write only the draft: %#v", database.execs)
	}
	args := database.execs[0].args
	if string(args[2].([]byte)) != string(body) || args[3] != false || args[5] != "user-a" {
		t.Fatalf("restore args = %#v, want the version body, no bundle without retention, the caller", args)
	}
	for _, call := range database.queryRows {
		if strings.Contains(call.query, "SET name = COALESCE") {
			t.Fatal("a restore must never publish")
		}
	}
	if strings.Contains(restoreDraftSQL, "content =") || strings.Contains(restoreDraftSQL, "version =") {
		t.Fatal("restoreDraftSQL must not touch the live text or re-point the version")
	}
	if args := database.actorCalls[0].args; args[2] != "draft_restored_from:v1" {
		t.Fatalf("the revision must say which version the draft came from: %#v", args)
	}
}

func TestRestoreOfAMissingVersionIsNotFound(t *testing.T) {
	withDrafts(t, true)
	database := &recordingDatabase{}
	database.nextRows = append(database.nextRows, boolRow{value: true})
	response := serveSkill(t, database, writeToken, http.MethodPost, "/api/v1/skills/sk-1/versions/9/restore", "")
	if response.Code != http.StatusNotFound || typedErrorCode(t, response.Body.Bytes()) != "skill_version_not_found" {
		t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
	}
	for _, path := range []string{"/api/v1/skills/sk-1/versions/0/restore", "/api/v1/skills/sk-1/versions/x/restore"} {
		if response := serveSkill(t, &recordingDatabase{}, writeToken, http.MethodPost, path, ""); response.Code != http.StatusBadRequest {
			t.Errorf("%s: status = %d, want 400", path, response.Code)
		}
	}
}

func TestDiscardDraftLeavesLiveText(t *testing.T) {
	withDrafts(t, true)
	database := &recordingDatabase{}
	response := serveSkill(t, database, writeToken, http.MethodDelete, "/api/v1/skills/sk-1/draft", "")
	if response.Code != http.StatusNoContent {
		t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
	}
	if len(database.execs) != 1 || database.execs[0].query != discardDraftSQL {
		t.Fatalf("a discard must clear only the draft: %#v", database.execs)
	}
	none := &recordingDatabase{execTag: "UPDATE 0"}
	if response := serveSkill(t, none, writeToken, http.MethodDelete, "/api/v1/skills/sk-1/draft", ""); response.Code != http.StatusNotFound {
		t.Fatalf("no draft: status = %d, want 404", response.Code)
	}
}

// Off, a skill delete leaves its bundles as before; with retention on they go in the same
// transaction (register D2-382).
func TestSkillDeleteRemovesBundlesOnlyWithRetention(t *testing.T) {
	for _, on := range []bool{false, true} {
		withDrafts(t, false)
		if on {
			t.Setenv(skillbundle.RetentionEnv, "true")
		}
		database := &recordingDatabase{}
		response := serveSkill(t, database, writeToken, http.MethodDelete, "/api/v1/skills/sk-1", "")
		if response.Code != http.StatusNoContent {
			t.Fatalf("retention %v: status = %d", on, response.Code)
		}
		removed := false
		for _, call := range database.execs {
			if strings.Contains(call.query, "DELETE FROM skill_bundles") {
				removed = true
			}
		}
		if removed != on {
			t.Fatalf("retention %v: bundles removed = %v", on, removed)
		}
	}
}

// The draft write and the publish here are the statements session-core's ignored DB tests apply
// (tests/skill_drafts.rs), so those tests prove what this service runs.
func TestDraftStatementsMatchTheSessionCoreTest(t *testing.T) {
	source, err := os.ReadFile(filepath.Join("..", "..", "..", "..", "..", "rust", "services", "session-core", "tests", "skill_drafts.rs"))
	if err != nil {
		t.Skipf("the session-core test is not beside this module: %v", err)
	}
	normalize := func(text string) string { return strings.Join(strings.Fields(text), " ") }
	test := normalize(string(source))
	for _, column := range []string{"name", "description", "content"} {
		line := column + " = COALESCE(draft->>'" + column + "', " + column + ")"
		if !strings.Contains(normalize(publishSkillSQL(false)), line) || !strings.Contains(test, line) {
			t.Errorf("publish and the session-core test disagree on %q", line)
		}
	}
	for _, column := range []string{"trigger_keywords", "trigger_file_patterns", "tool_restrictions", "settings"} {
		line := column + " = COALESCE(draft->'" + column + "', " + column + ")"
		if !strings.Contains(normalize(publishSkillSQL(false)), line) || !strings.Contains(test, line) {
			t.Errorf("publish and the session-core test disagree on %q", line)
		}
	}
	for _, fragment := range []string{"draft = COALESCE(draft, agent_skill_version_body(agent_skills))", "draft = NULL, draft_updated_at = NULL, draft_by = ''"} {
		if !strings.Contains(test, fragment) {
			t.Errorf("the session-core test lacks %q", fragment)
		}
	}
	if !strings.Contains(normalize(draftPatchSQL), "draft = COALESCE(draft, agent_skill_version_body(agent_skills))") {
		t.Error("draftPatchSQL must open the draft from the version body")
	}
}
