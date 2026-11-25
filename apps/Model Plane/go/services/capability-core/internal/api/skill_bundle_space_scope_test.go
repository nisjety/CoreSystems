package api

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/skillbundle"
)

// P2-go-15-rel1: a bundle upload or delete of a Space skill is a Space skill write, decided like the skill's own write.

const (
	spaceBundleTestOrg   = "org-a"
	spaceBundleTestSkill = "skill_1"
	spaceBundleTestSpace = "room-1"
	spaceBundleTestPatch = `{"content":"edited"}`
)

// spaceBundleRead is one read a bundle route made against the database.
type spaceBundleRead struct {
	query string
	args  []any
}

// spaceBundleDB is the database a bundle route runs against for one stored skill. The skill existence check
// (SKILL_BUNDLE_SCOPE_RULES) answers found. The scope read answers the stored scope and Space, no row when found is
// false, or readErr. Begin counts the writes the store starts, and every write runs in tx, so a refused request is seen
// to write nothing.
type spaceBundleDB struct {
	t       *testing.T
	tx      *bundleTestRecorder
	begins  int
	found   bool
	scope   string
	spaceID string
	readErr error
	reads   []spaceBundleRead
}

func (d *spaceBundleDB) Begin(context.Context) (pgx.Tx, error) {
	d.begins++
	return d.tx, nil
}

func (d *spaceBundleDB) Exec(context.Context, string, ...any) (pgconn.CommandTag, error) {
	d.t.Fatal("bundle writes must run in a transaction")
	return pgconn.CommandTag{}, nil
}

func (d *spaceBundleDB) Query(context.Context, string, ...any) (pgx.Rows, error) {
	d.t.Fatal("bundle reads are not part of this test")
	return nil, nil
}

func (d *spaceBundleDB) QueryRow(_ context.Context, query string, args ...any) pgx.Row {
	d.reads = append(d.reads, spaceBundleRead{query: query, args: append([]any(nil), args...)})
	switch {
	case strings.Contains(query, "SELECT EXISTS (SELECT 1 FROM agent_skills WHERE id = $1 AND org_id = $2)"):
		return spaceBundleExistsRow{value: d.found}
	case query == storedSkillScopeSQL:
		switch {
		case d.readErr != nil:
			return bundleTestRow{err: d.readErr}
		case !d.found:
			return bundleTestRow{err: pgx.ErrNoRows}
		}
		return bundleTestRow{values: []any{d.scope, d.spaceID}}
	}
	d.t.Fatalf("unexpected read in a bundle route: %s", query)
	return bundleTestRow{err: pgx.ErrNoRows}
}

// scopeReads returns the argument lists of the reads of the stored skill's scope.
func (d *spaceBundleDB) scopeReads() [][]any {
	var reads [][]any
	for _, read := range d.reads {
		if read.query == storedSkillScopeSQL {
			reads = append(reads, read.args)
		}
	}
	return reads
}

// spaceBundleExistsRow answers the skill existence check.
type spaceBundleExistsRow struct{ value bool }

func (row spaceBundleExistsRow) Scan(dest ...any) error {
	*dest[0].(*bool) = row.value
	return nil
}

// spaceBundleRoutes builds the bundle route as cmd/main.go builds it: SKILL_BUNDLE_SCOPE_RULES as scopeRules, the two
// Space switches as given, and the verifier wired when it is not nil. One handler serves every request of a test, so a
// replay reaches the same nonce cache.
func spaceBundleRoutes(t *testing.T, db skillbundle.DB, scopeRules, spaceScope, spaceWriteVerify bool, verifier *SpaceSkillDecisionVerifier) http.Handler {
	t.Helper()
	t.Setenv(skillSpaceScopeEnv, spaceBundleSwitch(spaceScope))
	t.Setenv(skillSpaceWriteVerifyEnv, spaceBundleSwitch(spaceWriteVerify))
	mux := http.NewServeMux()
	NewSkillBundlesHandler(db, true).WithScopeRules(scopeRules).WithSpaceSkillDecisionVerifier(verifier).Register(mux)
	return mux
}

// spaceBundleSwitch is the value a switch takes when it is on or off.
func spaceBundleSwitch(on bool) string {
	if on {
		return "true"
	}
	return ""
}

// serveSpaceBundle answers one request on the bundle route as the verified principal, with the decision header when
// decision is not empty.
func serveSpaceBundle(handler http.Handler, principal authctx.Principal, method string, body []byte, decision string) *httptest.ResponseRecorder {
	request := httptest.NewRequest(method, "/api/v1/skills/"+spaceBundleTestSkill+"/bundle", bytes.NewReader(body))
	request = request.WithContext(authctx.ContextWithPrincipal(request.Context(), principal))
	if decision != "" {
		request.Header.Set(spaceSkillDecisionHeader, decision)
	}
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)
	return response
}

// spaceBundleDigest is the digest a decision for a bundle write must carry: sha256 of the bytes as sent. It is spelled
// out here, not read back from the code under test, so the rule itself is pinned.
func spaceBundleDigest(body []byte) string {
	sum := sha256.Sum256(body)
	return "sha256:" + hex.EncodeToString(sum[:])
}

// spaceBundleIntent is the write a decision for one bundle write must bind: the verified person, the Space the stored
// row names, the operation, the skill and the digest of the body the route receives.
func spaceBundleIntent(operation, skillID string, body []byte) SkillWriteIntent {
	return SkillWriteIntent{OrgID: spaceBundleTestOrg, SpaceRef: spaceBundleTestSpace, SubjectID: "user-admin",
		Operation: operation, SkillID: skillID, RequestDigest: spaceBundleDigest(body)}
}

// With SKILL_SPACE_SCOPE and SKILL_SPACE_WRITE_VERIFY on, a bundle upload or delete of a Space skill needs a decision that
// verifies for exactly that write: the operation, the skill, the Space the stored row names, the person and the body. Each
// refusal answers its typed code, and nothing is written.
func TestSpaceSkillBundleWritesNeedAVerifiedDecision(t *testing.T) {
	upload := bundleTestZip(t, map[string]string{"SKILL.md": bundleTestSkillMD})
	otherUpload := bundleTestZip(t, map[string]string{"SKILL.md": bundleTestSkillMD, "notes.md": "changed\n"})
	verifier := newTestSpaceSkillVerifier(t)

	patchDigest, err := spaceSkillRequestDigest(spaceSkillOperationUpdate, []byte(spaceBundleTestPatch))
	if err != nil {
		t.Fatal(err)
	}
	patchText := spaceBundleIntent(spaceSkillOperationUpdate, spaceBundleTestSkill, nil)
	patchText.RequestDigest = patchDigest
	otherSpace := spaceBundleIntent(spaceSkillOperationUpdate, spaceBundleTestSkill, upload)
	otherSpace.SpaceRef = "room-2"
	expired := spaceSkillTestDecision(t, spaceBundleIntent(spaceSkillOperationUpdate, spaceBundleTestSkill, upload), time.Now().Add(-2*time.Minute))
	zdr := spaceSkillTestDecision(t, spaceBundleIntent(spaceSkillOperationUpdate, spaceBundleTestSkill, upload), time.Now())
	zdr["zero_data_retention"] = true

	type refusal struct {
		name, method, decision string
		body                   []byte
		status                 int
		code                   string
	}
	const invalid, required = spaceSkillDecisionInvalidCode, spaceSkillDecisionRequiredCode
	cases := []refusal{
		{"upload without the header", http.MethodPut, "", upload, http.StatusForbidden, required},
		{"upload for another skill", http.MethodPut,
			validSpaceSkillToken(t, spaceBundleIntent(spaceSkillOperationUpdate, "skill_2", upload)), upload, http.StatusForbidden, invalid},
		{"upload with another body", http.MethodPut,
			validSpaceSkillToken(t, spaceBundleIntent(spaceSkillOperationUpdate, spaceBundleTestSkill, otherUpload)), upload, http.StatusForbidden, invalid},
		{"upload for another Space", http.MethodPut, validSpaceSkillToken(t, otherSpace), upload, http.StatusForbidden, invalid},
		{"upload with a decision for the skill's text", http.MethodPut,
			validSpaceSkillToken(t, patchText), upload, http.StatusForbidden, invalid},
		{"upload with a skill delete decision", http.MethodPut,
			validSpaceSkillToken(t, spaceBundleIntent(spaceSkillOperationDelete, spaceBundleTestSkill, upload)), upload, http.StatusForbidden, invalid},
		{"upload with an expired decision", http.MethodPut,
			signSpaceSkillDecision(t, spaceSkillTestPrivate, spaceSkillTestKeyID, expired), upload, http.StatusForbidden, invalid},
		{"upload signed by another key", http.MethodPut,
			signSpaceSkillDecision(t, spaceSkillOtherKey, spaceSkillTestKeyID, spaceSkillTestDecision(t, spaceBundleIntent(spaceSkillOperationUpdate, spaceBundleTestSkill, upload), time.Now())),
			upload, http.StatusForbidden, invalid},
		{"upload with a ZDR decision", http.MethodPut,
			signSpaceSkillDecision(t, spaceSkillTestPrivate, spaceSkillTestKeyID, zdr), upload, http.StatusPreconditionFailed, "zdr_feature_not_supported"},
		{"delete without the header", http.MethodDelete, "", nil, http.StatusForbidden, required},
		{"delete with a skill delete decision", http.MethodDelete,
			validSpaceSkillToken(t, spaceBundleIntent(spaceSkillOperationDelete, spaceBundleTestSkill, nil)), nil, http.StatusForbidden, invalid},
		{"delete with an upload's decision", http.MethodDelete,
			validSpaceSkillToken(t, spaceBundleIntent(spaceSkillOperationUpdate, spaceBundleTestSkill, upload)), nil, http.StatusForbidden, invalid},
		{"delete for another skill", http.MethodDelete,
			validSpaceSkillToken(t, spaceBundleIntent(spaceSkillOperationUpdate, "skill_2", nil)), nil, http.StatusForbidden, invalid},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			db := &spaceBundleDB{t: t, tx: &bundleTestRecorder{t: t}, found: true, scope: spaceSkillScope, spaceID: spaceBundleTestSpace}
			handler := spaceBundleRoutes(t, db, true, true, true, verifier)
			response := serveSpaceBundle(handler, bundleTestAdmin(), test.method, test.body, test.decision)
			requireTypedRefusal(t, response, test.status, test.code)
			if db.begins != 0 || db.tx.committed || len(db.tx.execs) != 0 {
				t.Fatalf("a refused bundle write must write nothing: begins=%d committed=%v execs=%d", db.begins, db.tx.committed, len(db.tx.execs))
			}
		})
	}
}

// A bundle upload or delete of a Space skill with a decision that verifies for it is made, and the stored skill's scope is
// read for the Space the decision must name.
func TestSpaceSkillBundleWriteWithAVerifiedDecision(t *testing.T) {
	upload := bundleTestZip(t, map[string]string{"SKILL.md": bundleTestSkillMD})

	// Each subtest has its own verifier: a decision is single-use per replica, and two decisions made within one clock
	// tick share a nonce on a coarse clock.
	t.Run("upload", func(t *testing.T) {
		verifier := newTestSpaceSkillVerifier(t)
		tx := &bundleTestRecorder{t: t}
		db := &spaceBundleDB{t: t, tx: tx, found: true, scope: spaceSkillScope, spaceID: spaceBundleTestSpace}
		handler := spaceBundleRoutes(t, db, true, true, true, verifier)
		decision := validSpaceSkillToken(t, spaceBundleIntent(spaceSkillOperationUpdate, spaceBundleTestSkill, upload))
		response := serveSpaceBundle(handler, bundleTestAdmin(), http.MethodPut, upload, decision)
		if response.Code != http.StatusCreated {
			t.Fatalf("upload with a verified decision = %d, want 201 (%s)", response.Code, response.Body.String())
		}
		if !tx.committed {
			t.Fatal("the verified upload did not commit")
		}
		reads := db.scopeReads()
		if len(reads) != 1 || reads[0][0] != spaceBundleTestSkill || reads[0][1] != spaceBundleTestOrg {
			t.Fatalf("the stored skill's scope must be read for the skill of the org: %#v", reads)
		}
	})

	t.Run("delete", func(t *testing.T) {
		verifier := newTestSpaceSkillVerifier(t)
		tx := &bundleTestRecorder{t: t, deleteHit: true}
		db := &spaceBundleDB{t: t, tx: tx, found: true, scope: spaceSkillScope, spaceID: spaceBundleTestSpace}
		handler := spaceBundleRoutes(t, db, true, true, true, verifier)
		decision := validSpaceSkillToken(t, spaceBundleIntent(spaceSkillOperationUpdate, spaceBundleTestSkill, nil))
		response := serveSpaceBundle(handler, bundleTestAdmin(), http.MethodDelete, nil, decision)
		if response.Code != http.StatusNoContent {
			t.Fatalf("delete with a verified decision = %d, want 204 (%s)", response.Code, response.Body.String())
		}
		if !tx.committed {
			t.Fatal("the verified delete did not commit")
		}
	})
}

// A decision for a bundle write is single-use on this replica: the second upload that presents it is refused as replayed
// and writes nothing.
func TestSpaceSkillBundleDecisionIsSingleUse(t *testing.T) {
	upload := bundleTestZip(t, map[string]string{"SKILL.md": bundleTestSkillMD})
	tx := &bundleTestRecorder{t: t}
	db := &spaceBundleDB{t: t, tx: tx, found: true, scope: spaceSkillScope, spaceID: spaceBundleTestSpace}
	handler := spaceBundleRoutes(t, db, true, true, true, newTestSpaceSkillVerifier(t))
	decision := validSpaceSkillToken(t, spaceBundleIntent(spaceSkillOperationUpdate, spaceBundleTestSkill, upload))

	if first := serveSpaceBundle(handler, bundleTestAdmin(), http.MethodPut, upload, decision); first.Code != http.StatusCreated {
		t.Fatalf("first upload = %d, want 201 (%s)", first.Code, first.Body.String())
	}
	replay := serveSpaceBundle(handler, bundleTestAdmin(), http.MethodPut, upload, decision)
	requireTypedRefusal(t, replay, http.StatusConflict, spaceSkillDecisionReplayedCode)
	if db.begins != 1 {
		t.Fatalf("the replayed upload must write nothing: begins=%d, want 1", db.begins)
	}
}

// With either Space switch off, a bundle of a Space skill is written as it was before this fix: no decision is asked for
// and the skill's scope is not read.
func TestSpaceSkillBundleWritesAreUnchangedWithASwitchOff(t *testing.T) {
	upload := bundleTestZip(t, map[string]string{"SKILL.md": bundleTestSkillMD})
	for _, test := range []struct {
		name                         string
		spaceScope, spaceWriteVerify bool
	}{
		{name: "both off"},
		{name: "scope on, verify off", spaceScope: true},
		{name: "scope off, verify on", spaceWriteVerify: true},
	} {
		t.Run(test.name, func(t *testing.T) {
			verifier := newTestSpaceSkillVerifier(t)
			tx := &bundleTestRecorder{t: t}
			db := &spaceBundleDB{t: t, tx: tx, found: true, scope: spaceSkillScope, spaceID: spaceBundleTestSpace}
			handler := spaceBundleRoutes(t, db, true, test.spaceScope, test.spaceWriteVerify, verifier)
			response := serveSpaceBundle(handler, bundleTestAdmin(), http.MethodPut, upload, "")
			if response.Code != http.StatusCreated || !tx.committed {
				t.Fatalf("upload with a switch off = %d committed=%v, want 201 committed (%s)", response.Code, tx.committed, response.Body.String())
			}
			if len(db.scopeReads()) != 0 {
				t.Fatalf("the skill's scope was read with a switch off: %#v", db.reads)
			}

			deleteTx := &bundleTestRecorder{t: t, deleteHit: true}
			deleteDB := &spaceBundleDB{t: t, tx: deleteTx, found: true, scope: spaceSkillScope, spaceID: spaceBundleTestSpace}
			deleteHandler := spaceBundleRoutes(t, deleteDB, true, test.spaceScope, test.spaceWriteVerify, verifier)
			response = serveSpaceBundle(deleteHandler, bundleTestAdmin(), http.MethodDelete, nil, "")
			if response.Code != http.StatusNoContent || !deleteTx.committed {
				t.Fatalf("delete with a switch off = %d committed=%v, want 204 committed (%s)", response.Code, deleteTx.committed, response.Body.String())
			}
			if len(deleteDB.scopeReads()) != 0 {
				t.Fatalf("the skill's scope was read on delete with a switch off: %#v", deleteDB.reads)
			}
		})
	}
}

// With both switches on and no verifier wired, a bundle write of a Space skill is refused as the skill routes refuse a
// Space skill they cannot verify: space_skill_decision_required, and nothing written.
func TestSpaceSkillBundleWritesAreRefusedWithoutAVerifier(t *testing.T) {
	upload := bundleTestZip(t, map[string]string{"SKILL.md": bundleTestSkillMD})
	decision := validSpaceSkillToken(t, spaceBundleIntent(spaceSkillOperationUpdate, spaceBundleTestSkill, upload))
	for _, test := range []struct {
		name, method, decision string
		body                   []byte
	}{
		{"upload", http.MethodPut, decision, upload},
		{"delete", http.MethodDelete, decision, nil},
	} {
		t.Run(test.name, func(t *testing.T) {
			tx := &bundleTestRecorder{t: t, deleteHit: true}
			db := &spaceBundleDB{t: t, tx: tx, found: true, scope: spaceSkillScope, spaceID: spaceBundleTestSpace}
			handler := spaceBundleRoutes(t, db, true, true, true, nil)
			response := serveSpaceBundle(handler, bundleTestAdmin(), test.method, test.body, test.decision)
			requireTypedRefusal(t, response, http.StatusForbidden, spaceSkillDecisionRequiredCode)
			if db.begins != 0 || tx.committed {
				t.Fatalf("a Space skill bundle write without a verifier must write nothing: begins=%d committed=%v", db.begins, tx.committed)
			}
		})
	}
}

// With both switches on, a bundle of a skill that is not a Space skill, or of no skill row under no scope rules, is written
// as before: no decision is asked for.
func TestSpaceSkillBundleWritesOfOtherSkillsAreUnchanged(t *testing.T) {
	upload := bundleTestZip(t, map[string]string{"SKILL.md": bundleTestSkillMD})
	t.Run("an org skill", func(t *testing.T) {
		tx := &bundleTestRecorder{t: t}
		db := &spaceBundleDB{t: t, tx: tx, found: true, scope: "org"}
		handler := spaceBundleRoutes(t, db, true, true, true, newTestSpaceSkillVerifier(t))
		if response := serveSpaceBundle(handler, bundleTestAdmin(), http.MethodPut, upload, ""); response.Code != http.StatusCreated || !tx.committed {
			t.Fatalf("upload of an org skill = %d committed=%v, want 201 committed (%s)", response.Code, tx.committed, response.Body.String())
		}
		deleteTx := &bundleTestRecorder{t: t, deleteHit: true}
		deleteDB := &spaceBundleDB{t: t, tx: deleteTx, found: true, scope: "org"}
		deleteHandler := spaceBundleRoutes(t, deleteDB, true, true, true, newTestSpaceSkillVerifier(t))
		if response := serveSpaceBundle(deleteHandler, bundleTestAdmin(), http.MethodDelete, nil, ""); response.Code != http.StatusNoContent || !deleteTx.committed {
			t.Fatalf("delete of an org skill = %d committed=%v, want 204 committed (%s)", response.Code, deleteTx.committed, response.Body.String())
		}
	})
	t.Run("no skill row", func(t *testing.T) {
		tx := &bundleTestRecorder{t: t}
		db := &spaceBundleDB{t: t, tx: tx, found: false}
		handler := spaceBundleRoutes(t, db, false, true, true, newTestSpaceSkillVerifier(t))
		if response := serveSpaceBundle(handler, bundleTestAdmin(), http.MethodPut, upload, ""); response.Code != http.StatusCreated {
			t.Fatalf("upload with no skill row = %d, want 201 as before (%s)", response.Code, response.Body.String())
		}
		deleteTx := &bundleTestRecorder{t: t}
		deleteDB := &spaceBundleDB{t: t, tx: deleteTx, found: false}
		deleteHandler := spaceBundleRoutes(t, deleteDB, false, true, true, newTestSpaceSkillVerifier(t))
		if response := serveSpaceBundle(deleteHandler, bundleTestAdmin(), http.MethodDelete, nil, ""); response.Code != http.StatusNotFound {
			t.Fatalf("delete with no skill row = %d, want 404 as before (%s)", response.Code, response.Body.String())
		}
	})
}

// A failed read of the stored skill's scope answers the database error and writes nothing.
func TestSpaceSkillBundleScopeReadFailureWritesNothing(t *testing.T) {
	upload := bundleTestZip(t, map[string]string{"SKILL.md": bundleTestSkillMD})
	tx := &bundleTestRecorder{t: t}
	db := &spaceBundleDB{t: t, tx: tx, found: true, readErr: errors.New("connection reset")}
	handler := spaceBundleRoutes(t, db, true, true, true, newTestSpaceSkillVerifier(t))
	decision := validSpaceSkillToken(t, spaceBundleIntent(spaceSkillOperationUpdate, spaceBundleTestSkill, upload))
	response := serveSpaceBundle(handler, bundleTestAdmin(), http.MethodPut, upload, decision)
	if response.Code != http.StatusInternalServerError || !strings.Contains(response.Body.String(), "database unavailable") {
		t.Fatalf("scope read failure = %d %s, want 500 database unavailable", response.Code, response.Body.String())
	}
	if db.begins != 0 || tx.committed {
		t.Fatalf("a failed scope read must write nothing: begins=%d committed=%v", db.begins, tx.committed)
	}
}

// A bundle write's decision binds the digest of the bytes as sent: a zip is not canonicalised, and a bundle delete's body is
// empty, so its digest is that of zero bytes, as a skill delete's is.
func TestSpaceSkillBundleDigestIsOfTheBytesAsSent(t *testing.T) {
	upload := bundleTestZip(t, map[string]string{"SKILL.md": bundleTestSkillMD})
	if got, want := spaceSkillBundleDigest(upload), spaceBundleDigest(upload); got != want {
		t.Fatalf("upload digest = %s, want %s", got, want)
	}
	if got, want := spaceSkillBundleDigest(nil), spaceBundleDigest(nil); got != want {
		t.Fatalf("delete digest = %s, want %s", got, want)
	}
	deleteDigest, err := spaceSkillRequestDigest(spaceSkillOperationDelete, nil)
	if err != nil {
		t.Fatal(err)
	}
	if spaceSkillBundleDigest(nil) != deleteDigest {
		t.Fatalf("a bundle delete's digest %s must equal a skill delete's %s", spaceSkillBundleDigest(nil), deleteDigest)
	}
}
