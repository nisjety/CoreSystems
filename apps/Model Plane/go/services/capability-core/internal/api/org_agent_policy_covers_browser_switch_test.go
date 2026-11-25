package api

import (
	"context"
	"encoding/json"
	"net/http"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

// F-f09-13 (owner decision D-f09-3), P2-go-12-rel1: web.covers_browser is in force only while
// CAPABILITY_ORG_WEB_MODE_COVERS_BROWSER is the literal "true". With the switch off, a read shows the document a build
// without the key shows, its digest is that document's digest, and a write that names the key is refused as an unknown
// key before the registry is touched. The switch-on behaviour is pinned beside it.

const coversBrowserKey = "web.covers_browser"

// defaultsWithoutCoversBrowser is the defaults document of a build without web.covers_browser.
func defaultsWithoutCoversBrowser() map[string]any {
	settings := orgAgentPolicyDefaults()
	delete(settings, coversBrowserKey)
	return settings
}

// orgAgentRevisionRow answers a write's RETURNING revision.
type orgAgentRevisionRow struct{ revision int64 }

func (r orgAgentRevisionRow) Scan(dest ...any) error {
	*dest[0].(*int64) = r.revision
	return nil
}

// orgAgentRecordingDB answers each read and write with the row or rows it is given, and keeps the arguments of the last
// call, so a test can read the document and the digest a write sends to the registry.
type orgAgentRecordingDB struct {
	t    *testing.T
	row  pgx.Row
	rows pgx.Rows
	args []any
}

func (d *orgAgentRecordingDB) Exec(context.Context, string, ...any) (pgconn.CommandTag, error) {
	d.t.Helper()
	d.t.Fatal("the org agent policy never issues a plain Exec")
	return pgconn.CommandTag{}, nil
}

func (d *orgAgentRecordingDB) Query(_ context.Context, _ string, args ...any) (pgx.Rows, error) {
	d.args = args
	return d.rows, nil
}

func (d *orgAgentRecordingDB) QueryRow(_ context.Context, _ string, args ...any) pgx.Row {
	d.args = args
	return d.row
}

// orgAgentHistoryRows lists stored documents, newest first, scanned the way the history read scans them.
type orgAgentHistoryRows struct {
	pgx.Rows
	docs []string
	next int
}

func (r *orgAgentHistoryRows) Next() bool {
	r.next++
	return r.next <= len(r.docs)
}

func (r *orgAgentHistoryRows) Scan(dest ...any) error {
	*dest[0].(*int64) = int64(len(r.docs) - r.next + 1)
	*dest[1].(*string) = "digest"
	*dest[2].(*string) = "user-admin"
	*dest[3].(*time.Time) = time.Unix(0, 0)
	*dest[4].(*string) = r.docs[r.next-1]
	return nil
}

func (r *orgAgentHistoryRows) Close()     {}
func (r *orgAgentHistoryRows) Err() error { return nil }

// The switch reads only the literal "true", as ORG_AGENT_POLICY_API does.
func TestOrgAgentPolicyCoversBrowserSwitchIsOnlyTheLiteralTrue(t *testing.T) {
	for raw, want := range map[string]bool{"true": true, "1": false, "TRUE": false, " true": false, "yes": false, "": false} {
		if got := OrgWebCoversBrowserFromLookup(func(string) string { return raw }); got != want {
			t.Errorf("CAPABILITY_ORG_WEB_MODE_COVERS_BROWSER=%q on = %v, want %v", raw, got, want)
		}
	}
}

// With the switch off, the document read is the one a build without the key reads: no web.covers_browser, the same
// keys, the digest of the document without it, and the effective read lists no entry for it.
func TestOrgAgentPolicyCoversBrowserOffReadsTheDocumentWithoutTheKey(t *testing.T) {
	member := orgAgentMember()
	db := orgAgentFakeDB{t: t, row: orgAgentErrRow{err: pgx.ErrNoRows}}
	want := defaultsWithoutCoversBrowser()

	response, _ := serveOrgAgentPolicy(t, true, db, &member, http.MethodGet, orgAgentPolicyPath, "", "")
	if response.Code != http.StatusOK {
		t.Fatalf("GET = %d %s", response.Code, response.Body.String())
	}
	var view orgAgentPolicyView
	if err := json.Unmarshal(response.Body.Bytes(), &view); err != nil {
		t.Fatal(err)
	}
	if _, shown := view.Settings[coversBrowserKey]; shown || len(view.Settings) != len(want) {
		t.Fatalf("settings with the switch off = %v, want the %d keys of a build without the key", view.Settings, len(want))
	}
	wantDigest, _, err := orgAgentPolicyDigest(want)
	if err != nil {
		t.Fatal(err)
	}
	if view.Digest != wantDigest {
		t.Fatalf("digest with the switch off = %s, want the digest without the key %s", view.Digest, wantDigest)
	}

	effective, _ := serveOrgAgentPolicy(t, true, db, &member, http.MethodGet, orgAgentPolicyPath+"/effective?user=me", "", "")
	if effective.Code != http.StatusOK {
		t.Fatalf("effective = %d %s", effective.Code, effective.Body.String())
	}
	var listed struct {
		Settings []orgAgentEffectiveSetting `json:"settings"`
	}
	if err := json.Unmarshal(effective.Body.Bytes(), &listed); err != nil {
		t.Fatal(err)
	}
	if len(listed.Settings) != len(want) {
		t.Fatalf("effective lists %d keys with the switch off, want %d", len(listed.Settings), len(want))
	}
	for _, row := range listed.Settings {
		if row.Key == coversBrowserKey {
			t.Fatalf("effective names %s with the switch off: %+v", coversBrowserKey, row)
		}
	}
}

// A document stored while the switch was on keeps its digest, but the switch-off reads leave the key out of it: the
// document read, the effective read and every history entry.
func TestOrgAgentPolicyCoversBrowserOffHidesAStoredKeyFromEveryRead(t *testing.T) {
	member := orgAgentMember()
	stored := `{"mode_cap":"read_only","web.covers_browser":false}`

	db := orgAgentFakeDB{t: t, row: orgAgentDocRow{doc: stored}}
	response, _ := serveOrgAgentPolicy(t, true, db, &member, http.MethodGet, orgAgentPolicyPath, "", "")
	var view orgAgentPolicyView
	if err := json.Unmarshal(response.Body.Bytes(), &view); err != nil {
		t.Fatal(err)
	}
	if _, shown := view.Settings[coversBrowserKey]; shown || view.Settings["mode_cap"] != "read_only" {
		t.Fatalf("stored document read with the switch off = %v, want mode_cap only", view.Settings)
	}

	effective, _ := serveOrgAgentPolicy(t, true, db, &member, http.MethodGet, orgAgentPolicyPath+"/effective?user=me", "", "")
	var listed struct {
		Settings []orgAgentEffectiveSetting `json:"settings"`
	}
	if err := json.Unmarshal(effective.Body.Bytes(), &listed); err != nil {
		t.Fatal(err)
	}
	for _, row := range listed.Settings {
		if row.Key == coversBrowserKey {
			t.Fatalf("effective names %s with the switch off: %+v", coversBrowserKey, row)
		}
	}

	history := &orgAgentRecordingDB{t: t, rows: &orgAgentHistoryRows{docs: []string{stored}}}
	historyResponse, _ := serveOrgAgentPolicy(t, true, history, &member, http.MethodGet, orgAgentPolicyPath+"/history", "", "")
	if historyResponse.Code != http.StatusOK {
		t.Fatalf("history = %d %s", historyResponse.Code, historyResponse.Body.String())
	}
	var entries []orgAgentHistoryEntry
	if err := json.Unmarshal(historyResponse.Body.Bytes(), &entries); err != nil {
		t.Fatal(err)
	}
	if len(entries) != 1 {
		t.Fatalf("history entries = %d, want 1", len(entries))
	}
	if _, shown := entries[0].Settings[coversBrowserKey]; shown {
		t.Fatalf("history shows %s with the switch off: %v", coversBrowserKey, entries[0].Settings)
	}
}

// With the switch off, a write that names the key is refused as an unknown key before the registry is touched: the answer
// a build without the key gives.
func TestOrgAgentPolicyCoversBrowserOffRefusesAWriteThatNamesTheKey(t *testing.T) {
	admin := orgAgentAdmin()
	for _, body := range []string{
		`{"settings":{"web.covers_browser":false}}`,
		`{"settings":{"web.covers_browser":true}}`,
		`{"settings":{"web.covers_browser":"maybe"}}`,
	} {
		response, _ := serveOrgAgentPolicy(t, true, orgAgentFakeDB{t: t}, &admin, http.MethodPut, orgAgentPolicyPath, "0", body)
		if response.Code != http.StatusBadRequest || orgAgentErrorCode(t, response) != "unknown_key" {
			t.Fatalf("%s with the switch off = %d %s, want 400 unknown_key", body, response.Code, response.Body.String())
		}
	}
}

// With the switch off, a write that does not name the key stores the document a build without the key stores, at the
// digest that document has, and the response shows no key.
func TestOrgAgentPolicyCoversBrowserOffWriteKeepsTheDigestWithoutTheKey(t *testing.T) {
	admin := orgAgentAdmin()
	db := &orgAgentRecordingDB{t: t, row: orgAgentRevisionRow{revision: 1}}
	response, _ := serveOrgAgentPolicy(t, true, db, &admin, http.MethodPut, orgAgentPolicyPath, "0", `{"settings":{"mode_cap":"read_only"}}`)
	if response.Code != http.StatusOK {
		t.Fatalf("PUT = %d %s", response.Code, response.Body.String())
	}

	want := defaultsWithoutCoversBrowser()
	want["mode_cap"] = "read_only"
	wantDigest, wantDoc, err := orgAgentPolicyDigest(want)
	if err != nil {
		t.Fatal(err)
	}
	// The write's arguments are the org, the expected revision, the document, the digest, then the actor and the ids.
	doc, docOK := db.args[2].(string)
	digest, digestOK := db.args[3].(string)
	if !docOK || !digestOK {
		t.Fatalf("the write's arguments are %v, want a document and a digest as strings", db.args)
	}
	if strings.Contains(doc, coversBrowserKey) {
		t.Fatalf("the document stored with the switch off names %s: %s", coversBrowserKey, doc)
	}
	if doc != string(wantDoc) || digest != wantDigest {
		t.Fatalf("stored %s at %s, want %s at %s", doc, digest, wantDoc, wantDigest)
	}
	var view orgAgentPolicyView
	if err := json.Unmarshal(response.Body.Bytes(), &view); err != nil {
		t.Fatal(err)
	}
	if _, shown := view.Settings[coversBrowserKey]; shown {
		t.Fatalf("the write's response shows %s with the switch off", coversBrowserKey)
	}
}

// With the switch on, the key is served with its default, listed as enforced, and a write names it: the document and
// its digest carry the value written.
func TestOrgAgentPolicyCoversBrowserOnIsServedAndAccepted(t *testing.T) {
	member := orgAgentMember()
	admin := orgAgentAdmin()
	empty := orgAgentFakeDB{t: t, row: orgAgentErrRow{err: pgx.ErrNoRows}}

	read, _ := serveOrgAgentPolicyWith(t, NewOrgAgentPolicyHandler(empty, true).WithWebCoversBrowser(true), &member, http.MethodGet, orgAgentPolicyPath, "", "")
	var view orgAgentPolicyView
	if err := json.Unmarshal(read.Body.Bytes(), &view); err != nil {
		t.Fatal(err)
	}
	if view.Settings[coversBrowserKey] != true || len(view.Settings) != len(orgAgentPolicySettings) {
		t.Fatalf("settings with the switch on = %v, want %s true among %d keys", view.Settings, coversBrowserKey, len(orgAgentPolicySettings))
	}
	defaultsDigest, _, err := orgAgentPolicyDigest(orgAgentPolicyDefaults())
	if err != nil {
		t.Fatal(err)
	}
	if view.Digest != defaultsDigest {
		t.Fatalf("digest with the switch on = %s, want the defaults' digest %s", view.Digest, defaultsDigest)
	}

	effective, _ := serveOrgAgentPolicyWith(t, NewOrgAgentPolicyHandler(empty, true).WithWebCoversBrowser(true), &member, http.MethodGet, orgAgentPolicyPath+"/effective?user=me", "", "")
	var listed struct {
		Settings []orgAgentEffectiveSetting `json:"settings"`
	}
	if err := json.Unmarshal(effective.Body.Bytes(), &listed); err != nil {
		t.Fatal(err)
	}
	found := false
	for _, row := range listed.Settings {
		if row.Key != coversBrowserKey {
			continue
		}
		found = true
		if row.Status != "enforced" || row.Source != "platform_default" || row.Value != true {
			t.Fatalf("%s listed as %+v with the switch on, want enforced, platform_default, true", coversBrowserKey, row)
		}
	}
	if !found {
		t.Fatalf("effective does not list %s with the switch on", coversBrowserKey)
	}

	recorder := &orgAgentRecordingDB{t: t, row: orgAgentRevisionRow{revision: 1}}
	put, _ := serveOrgAgentPolicyWith(t, NewOrgAgentPolicyHandler(recorder, true).WithWebCoversBrowser(true), &admin, http.MethodPut, orgAgentPolicyPath, "0", `{"settings":{"web.covers_browser":false}}`)
	if put.Code != http.StatusOK {
		t.Fatalf("PUT with the switch on = %d %s", put.Code, put.Body.String())
	}
	want := orgAgentPolicyDefaults()
	want[coversBrowserKey] = false
	wantDigest, wantDoc, err := orgAgentPolicyDigest(want)
	if err != nil {
		t.Fatal(err)
	}
	if doc, _ := recorder.args[2].(string); doc != string(wantDoc) {
		t.Fatalf("document stored with the switch on = %s, want %s", doc, wantDoc)
	}
	if digest, _ := recorder.args[3].(string); digest != wantDigest {
		t.Fatalf("digest stored with the switch on = %s, want %s", digest, wantDigest)
	}
}

// With the switch on, a stored document's key is read back as written, in the document read and in history.
func TestOrgAgentPolicyCoversBrowserOnReadsAStoredKeyBack(t *testing.T) {
	member := orgAgentMember()
	stored := `{"mode_cap":"read_only","web.covers_browser":false}`

	db := orgAgentFakeDB{t: t, row: orgAgentDocRow{doc: stored}}
	read, _ := serveOrgAgentPolicyWith(t, NewOrgAgentPolicyHandler(db, true).WithWebCoversBrowser(true), &member, http.MethodGet, orgAgentPolicyPath, "", "")
	var view orgAgentPolicyView
	if err := json.Unmarshal(read.Body.Bytes(), &view); err != nil {
		t.Fatal(err)
	}
	if view.Settings[coversBrowserKey] != false {
		t.Fatalf("stored %s read with the switch on = %v, want false", coversBrowserKey, view.Settings[coversBrowserKey])
	}

	history := &orgAgentRecordingDB{t: t, rows: &orgAgentHistoryRows{docs: []string{stored}}}
	historyResponse, _ := serveOrgAgentPolicyWith(t, NewOrgAgentPolicyHandler(history, true).WithWebCoversBrowser(true), &member, http.MethodGet, orgAgentPolicyPath+"/history", "", "")
	var entries []orgAgentHistoryEntry
	if err := json.Unmarshal(historyResponse.Body.Bytes(), &entries); err != nil {
		t.Fatal(err)
	}
	if len(entries) != 1 || entries[0].Settings[coversBrowserKey] != false {
		t.Fatalf("history with the switch on = %+v, want %s false", entries, coversBrowserKey)
	}
}

// The browser-switched row is pinned here as well as by the golden, so a change to its kind, order, default or
// fail-closed value is a deliberate one. Only this row is behind the browser switch.
func TestOrgAgentPolicyCoversBrowserRowIsPinned(t *testing.T) {
	var found []orgAgentSetting
	for _, def := range orgAgentPolicySettings {
		if def.key == coversBrowserKey {
			found = append(found, def)
		}
		if def.browserSwitch && def.key != coversBrowserKey {
			t.Fatalf("%s is behind the browser switch too", def.key)
		}
	}
	if len(found) != 1 {
		t.Fatalf("%d rows for %s, want one", len(found), coversBrowserKey)
	}
	row := found[0]
	if row.kind != "bool" || !reflect.DeepEqual(row.order, []string{"true", "false"}) || row.def != "true" || row.failClose != "true" ||
		row.reserved || len(row.reservedValues) != 0 || !row.browserSwitch {
		t.Fatalf("row = %+v, want an enforced true/false bool, default and fail-closed true, behind the browser switch", row)
	}
}
