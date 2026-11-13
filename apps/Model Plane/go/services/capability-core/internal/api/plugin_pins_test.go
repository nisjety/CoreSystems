package api

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/triodelab/model-plane/pkg/authctx"
)

// W4-PL-1 (supply-chain pins) and W4-PL-3 (catalogue and kill list), without a database. The
// fake records every statement, so a refusal can be shown to happen before anything is written.

type pluginFakeDB struct {
	execs    []string
	execArgs [][]any
	affected int64
	execErr  error
	row      func(dest ...any) error
}

func (d *pluginFakeDB) Exec(_ context.Context, sql string, args ...any) (pgconn.CommandTag, error) {
	d.execs = append(d.execs, sql)
	d.execArgs = append(d.execArgs, args)
	if d.execErr != nil {
		return pgconn.CommandTag{}, d.execErr
	}
	if d.affected == 0 {
		return pgconn.NewCommandTag("UPDATE 0"), nil
	}
	return pgconn.NewCommandTag("INSERT 0 1"), nil
}

func (d *pluginFakeDB) Query(context.Context, string, ...any) (pgx.Rows, error) {
	return nil, errors.New("unexpected query")
}

func (d *pluginFakeDB) QueryRow(context.Context, string, ...any) pgx.Row {
	return scanRowFunc(func(dest ...any) error {
		if d.row == nil {
			return pgx.ErrNoRows
		}
		return d.row(dest...)
	})
}

type scanRowFunc func(dest ...any) error

func (f scanRowFunc) Scan(dest ...any) error { return f(dest...) }

func pluginWriterPrincipal() authctx.Principal {
	return authctx.Principal{OrganizationID: "org-a", ActorID: "user-a", PrincipalType: "user",
		Scopes: []string{"capability:write", "org:admin"}, RetentionPolicyPresent: true}
}

func servePlugins(handler interface{ Register(*http.ServeMux) }, principal authctx.Principal, method, target, body string) *httptest.ResponseRecorder {
	mux := http.NewServeMux()
	handler.Register(mux)
	request := httptest.NewRequest(method, target, strings.NewReader(body))
	request = request.WithContext(authctx.ContextWithPrincipal(request.Context(), principal))
	recorder := httptest.NewRecorder()
	mux.ServeHTTP(recorder, request)
	return recorder
}

func typedCode(t *testing.T, recorder *httptest.ResponseRecorder) string {
	t.Helper()
	var envelope struct {
		Error struct {
			Code string `json:"code"`
		} `json:"error"`
	}
	_ = json.Unmarshal(recorder.Body.Bytes(), &envelope)
	return envelope.Error.Code
}

const (
	goodCommit  = "0123456789abcdef0123456789abcdef01234567"
	goodArchive = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
)

func TestPluginLoaderSwitchIsOnlyTheLiteralTrue(t *testing.T) {
	for value, want := range map[string]bool{"true": true, "": false, "TRUE": false, "1": false, "yes": false, " true": false} {
		got := PluginLoaderFromLookup(func(name string) string {
			if name != "PLUGIN_LOADER_ENABLED" {
				t.Fatalf("read %q", name)
			}
			return value
		})
		if got != want {
			t.Errorf("%q -> %t, want %t", value, got, want)
		}
	}
}

func TestCheckPluginSource(t *testing.T) {
	cases := []struct {
		name, kind, ref, pin, wantCode string
	}{
		{"git commit", "git_commit", "https://git.example/org/repo.git", goodCommit, ""},
		{"upper-case commit is canonicalised", "git_commit", "https://git.example/org/repo.git", strings.ToUpper(goodCommit), ""},
		{"archive digest", "archive", "https://files.example/p.tgz", goodArchive, ""},
		{"missing pin", "git_commit", "https://git.example/r", "", pluginSourceRequiredCode},
		{"missing ref", "git_commit", "", goodCommit, pluginSourceRequiredCode},
		{"branch name", "git_commit", "https://git.example/r", "main", pluginSourceInvalidCode},
		{"tag name", "git_commit", "https://git.example/r", "v1.2.3", pluginSourceInvalidCode},
		{"short sha", "git_commit", "https://git.example/r", goodCommit[:12], pluginSourceInvalidCode},
		{"archive pin on a git source", "git_commit", "https://git.example/r", goodArchive, pluginSourceInvalidCode},
		{"commit pin on an archive", "archive", "https://files.example/p.tgz", goodCommit, pluginSourceInvalidCode},
		{"unknown kind", "svn", "https://svn.example/r", goodCommit, pluginSourceInvalidCode},
		{"http url", "git_commit", "http://git.example/r", goodCommit, pluginSourceInvalidCode},
		{"credentials in url", "git_commit", "https://user:token@git.example/r", goodCommit, pluginSourceInvalidCode},
		{"query in url", "git_commit", "https://git.example/r?token=x", goodCommit, pluginSourceInvalidCode},
		{"fragment in url", "git_commit", "https://git.example/r#frag", goodCommit, pluginSourceInvalidCode},
		{"no host", "git_commit", "https:///r", goodCommit, pluginSourceInvalidCode},
		{"too long", "git_commit", "https://git.example/" + strings.Repeat("a", maxPluginSourceRefBytes), goodCommit, pluginSourceInvalidCode},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			src, refusal := checkPluginSource(tc.kind, tc.ref, tc.pin)
			if tc.wantCode == "" {
				if refusal != nil {
					t.Fatalf("refused: %+v", refusal)
				}
				if src.pin != strings.ToLower(tc.pin) {
					t.Fatalf("pin not canonical: %q", src.pin)
				}
				return
			}
			if refusal == nil || refusal.code != tc.wantCode || refusal.status != http.StatusUnprocessableEntity {
				t.Fatalf("refusal = %+v, want 422 %s", refusal, tc.wantCode)
			}
		})
	}
}

func TestPluginManifestDigestIsCanonical(t *testing.T) {
	a, okA := pluginManifestDigest(json.RawMessage(`{"b":1,"a":{"y":[1,2],"x":"<&>"}}`))
	b, okB := pluginManifestDigest(json.RawMessage("{\n  \"a\": {\"x\": \"<&>\", \"y\": [1, 2]},\n  \"b\": 1\n}"))
	if !okA || !okB || a != b {
		t.Fatalf("two spellings of one document: %q (%t) vs %q (%t)", a, okA, b, okB)
	}
	if len(a) != len("sha256:")+64 || !strings.HasPrefix(a, "sha256:") {
		t.Fatalf("digest shape %q", a)
	}
	// The digest pins the canonical text: sorted keys, no whitespace, no HTML escaping, numbers as written.
	want, _ := pluginManifestDigest(json.RawMessage(`{"a":{"x":"<&>","y":[1,2]},"b":1}`))
	if a != want {
		t.Fatalf("digest is not over the canonical form")
	}
	c, _ := pluginManifestDigest(json.RawMessage(`{"b":1.0,"a":{"y":[1,2],"x":"<&>"}}`))
	if c == a {
		t.Fatal("1.0 and 1 hash the same; numbers must be kept as written")
	}
	for _, raw := range []string{`[]`, `"x"`, `null`, `not json`, ``} {
		if _, ok := pluginManifestDigest(json.RawMessage(raw)); ok {
			t.Errorf("%q digested; only an object is a manifest", raw)
		}
	}
}

func TestPluginCreateWithTheSwitchOffIsInertCRUD(t *testing.T) {
	db := &pluginFakeDB{affected: 1}
	body := `{"name":"p","version":"1","enabled":true,"source_kind":"git_commit","source_ref":"https://g.example/r","source_pin":"main"}`
	recorder := servePlugins(NewPluginsHandler(db).WithSupplyChainPins(false), pluginWriterPrincipal(), http.MethodPost, "/api/v1/plugins", body)
	if recorder.Code != http.StatusCreated {
		t.Fatalf("status = %d, body = %s", recorder.Code, recorder.Body.String())
	}
	if len(db.execs) != 1 || strings.Contains(db.execs[0], "source_kind") || strings.Contains(db.execs[0], "plugin_kill_list") {
		t.Fatalf("switch off wrote %v", db.execs)
	}
	// Enabled and pinned are always written false on create.
	if db.execArgs[0][7] != false || db.execArgs[0][8] != false {
		t.Fatalf("enabled/pinned args = %v %v", db.execArgs[0][7], db.execArgs[0][8])
	}
}

func TestPluginCreateWithTheSwitchOnRefusesAnUnpinnedSourceBeforeWriting(t *testing.T) {
	for name, body := range map[string]string{
		"no source":    `{"name":"p","version":"1"}`,
		"branch pin":   `{"name":"p","version":"1","source_kind":"git_commit","source_ref":"https://g.example/r","source_pin":"main"}`,
		"bad manifest": `{"name":"p","version":"1","manifest_json":[1],"source_kind":"git_commit","source_ref":"https://g.example/r","source_pin":"` + goodCommit + `"}`,
		"http source":  `{"name":"p","version":"1","source_kind":"archive","source_ref":"http://f.example/p.tgz","source_pin":"` + goodArchive + `"}`,
	} {
		t.Run(name, func(t *testing.T) {
			db := &pluginFakeDB{affected: 1}
			recorder := servePlugins(NewPluginsHandler(db).WithSupplyChainPins(true), pluginWriterPrincipal(), http.MethodPost, "/api/v1/plugins", body)
			if recorder.Code != http.StatusUnprocessableEntity || len(db.execs) != 0 {
				t.Fatalf("status=%d code=%q writes=%d", recorder.Code, typedCode(t, recorder), len(db.execs))
			}
		})
	}
}

func TestPluginCreateWithTheSwitchOnComputesTheDigestAndRefusesAKilledOne(t *testing.T) {
	manifest := `{"b":1,"a":2}`
	body := `{"name":"p","version":"1","manifest_json":` + manifest + `,"manifest_digest":"sha256:` + strings.Repeat("f", 64) +
		`","source_kind":"git_commit","source_ref":"https://g.example/r","source_pin":"` + strings.ToUpper(goodCommit) + `"}`
	want, _ := pluginManifestDigest(json.RawMessage(manifest))

	db := &pluginFakeDB{affected: 1}
	recorder := servePlugins(NewPluginsHandler(db).WithSupplyChainPins(true), pluginWriterPrincipal(), http.MethodPost, "/api/v1/plugins", body)
	if recorder.Code != http.StatusCreated || len(db.execs) != 1 {
		t.Fatalf("status=%d body=%s", recorder.Code, recorder.Body.String())
	}
	args := db.execArgs[0]
	if args[11] != want || args[10] != goodCommit || args[8] != "git_commit" {
		t.Fatalf("digest/pin/kind args = %v %v %v; want the server's digest %s and the lower-case pin", args[11], args[10], args[8], want)
	}
	if !strings.Contains(db.execs[0], "plugin_kill_list") || !strings.Contains(db.execs[0], "FALSE, FALSE") {
		t.Fatal("the insert does not refuse a killed digest or does not start disabled and unpinned")
	}

	killed := &pluginFakeDB{affected: 0}
	recorder = servePlugins(NewPluginsHandler(killed).WithSupplyChainPins(true), pluginWriterPrincipal(), http.MethodPost, "/api/v1/plugins", body)
	if recorder.Code != http.StatusUnprocessableEntity || typedCode(t, recorder) != "plugin_killed" {
		t.Fatalf("killed digest: status=%d code=%q", recorder.Code, typedCode(t, recorder))
	}
}

func TestPluginPatchWithTheSwitchOn(t *testing.T) {
	t.Run("source is immutable", func(t *testing.T) {
		db := &pluginFakeDB{affected: 1}
		recorder := servePlugins(NewPluginsHandler(db).WithSupplyChainPins(true), pluginWriterPrincipal(), http.MethodPatch,
			"/api/v1/plugins/plugin_1", `{"source_pin":"`+goodCommit+`"}`)
		if recorder.Code != http.StatusConflict || typedCode(t, recorder) != "plugin_source_immutable" || len(db.execs) != 0 {
			t.Fatalf("status=%d code=%q writes=%d", recorder.Code, typedCode(t, recorder), len(db.execs))
		}
	})
	refusals := []struct {
		name                               string
		body                               string
		pinned, sourced, installed, killed bool
		wantStatus                         int
		wantCode                           string
	}{
		{"enable a killed plugin", `{"enabled":true}`, true, true, true, true, http.StatusUnprocessableEntity, "plugin_killed"},
		{"pin a killed plugin", `{"pinned":true}`, false, true, false, true, http.StatusUnprocessableEntity, "plugin_killed"},
		{"pin with no source", `{"pinned":true}`, false, false, false, false, http.StatusUnprocessableEntity, "plugin_source_required"},
		{"enable an uninstalled plugin", `{"enabled":true}`, true, true, false, false, http.StatusConflict, "plugin_not_installed"},
		{"enable an unpinned plugin", `{"enabled":true}`, false, true, true, false, http.StatusConflict, "plugin_not_installed"},
		{"enable while unpinning", `{"enabled":true,"pinned":false}`, true, true, true, false, http.StatusConflict, "plugin_not_installed"},
	}
	for _, tc := range refusals {
		t.Run(tc.name, func(t *testing.T) {
			db := &pluginFakeDB{affected: 0, row: func(dest ...any) error {
				*dest[0].(*bool), *dest[1].(*bool), *dest[2].(*bool), *dest[3].(*bool) = tc.pinned, tc.sourced, tc.installed, tc.killed
				return nil
			}}
			recorder := servePlugins(NewPluginsHandler(db).WithSupplyChainPins(true), pluginWriterPrincipal(), http.MethodPatch, "/api/v1/plugins/plugin_1", tc.body)
			if recorder.Code != tc.wantStatus || typedCode(t, recorder) != tc.wantCode {
				t.Fatalf("status=%d code=%q, want %d %s", recorder.Code, typedCode(t, recorder), tc.wantStatus, tc.wantCode)
			}
		})
	}
	t.Run("the guarded update carries the rules in its WHERE clause", func(t *testing.T) {
		db := &pluginFakeDB{affected: 1}
		recorder := servePlugins(NewPluginsHandler(db).WithSupplyChainPins(true), pluginWriterPrincipal(), http.MethodPatch, "/api/v1/plugins/plugin_1", `{"enabled":true}`)
		if recorder.Code != http.StatusOK || len(db.execs) != 1 {
			t.Fatalf("status=%d", recorder.Code)
		}
		sql := db.execs[0]
		for _, must := range []string{"installed_at IS NOT NULL", "plugin_kill_list", "source_pin IS NOT NULL", "WHEN $2::boolean IS FALSE THEN FALSE"} {
			if !strings.Contains(sql, must) {
				t.Errorf("enable update lost %q", must)
			}
		}
	})
}

func TestPluginCatalogueRoutesAreAbsentWhileTheSwitchIsOff(t *testing.T) {
	handler := NewPluginCatalogHandler(&pluginFakeDB{}).WithLoaderEnabled(false).WithChildStores(nil, nil)
	for _, route := range [][2]string{
		{http.MethodGet, "/api/v1/plugin-catalog"},
		{http.MethodPost, "/api/v1/plugin-catalog/plugin_1/approve"},
		{http.MethodPost, "/api/v1/plugin-kill-list"},
	} {
		if recorder := servePlugins(handler, pluginWriterPrincipal(), route[0], route[1], `{}`); recorder.Code != http.StatusNotFound {
			t.Fatalf("%s %s answered %d with the switch off", route[0], route[1], recorder.Code)
		}
	}
}

func TestPluginCatalogueWritesAreRefusedBeforeTheDatabase(t *testing.T) {
	zdr := pluginWriterPrincipal()
	zdr.ZeroDataRetention = true
	unstated := pluginWriterPrincipal()
	unstated.RetentionPolicyPresent = false
	member := pluginWriterPrincipal()
	member.Scopes = []string{"capability:write"}
	service := pluginWriterPrincipal()
	service.PrincipalType = "service"
	tenantPlatform := pluginWriterPrincipal()
	tenantPlatform.Scopes = []string{"capability:write", "capability:global:write", "org:admin"}
	digest := `"digest":"` + goodArchive + `"`

	cases := []struct {
		name      string
		principal authctx.Principal
		method    string
		target    string
		body      string
		want      int
	}{
		{"zdr admin approves", zdr, http.MethodPost, "/api/v1/plugin-catalog/plugin_1/approve", ``, http.StatusForbidden},
		{"unstated posture approves", unstated, http.MethodPost, "/api/v1/plugin-catalog/plugin_1/approve", ``, http.StatusForbidden},
		{"member approves", member, http.MethodPost, "/api/v1/plugin-catalog/plugin_1/approve", ``, http.StatusForbidden},
		{"service approves", service, http.MethodPost, "/api/v1/plugin-catalog/plugin_1/approve", ``, http.StatusForbidden},
		{"zdr admin kills", zdr, http.MethodPost, "/api/v1/plugin-kill-list", `{` + digest + `}`, http.StatusForbidden},
		{"member kills", member, http.MethodPost, "/api/v1/plugin-kill-list", `{` + digest + `}`, http.StatusForbidden},
		{"org admin kills globally", pluginWriterPrincipal(), http.MethodPost, "/api/v1/plugin-kill-list", `{` + digest + `,"scope":"global"}`, http.StatusForbidden},
		{"tenant with the global scope kills globally", tenantPlatform, http.MethodPost, "/api/v1/plugin-kill-list", `{` + digest + `,"scope":"global"}`, http.StatusForbidden},
		{"branch-like digest", pluginWriterPrincipal(), http.MethodPost, "/api/v1/plugin-kill-list", `{"digest":"main"}`, http.StatusUnprocessableEntity},
		{"unknown scope", pluginWriterPrincipal(), http.MethodPost, "/api/v1/plugin-kill-list", `{` + digest + `,"scope":"space"}`, http.StatusUnprocessableEntity},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			db := &pluginFakeDB{affected: 1}
			handler := NewPluginCatalogHandler(db).WithLoaderEnabled(true).WithChildStores(nil, nil)
			recorder := servePlugins(handler, tc.principal, tc.method, tc.target, tc.body)
			if recorder.Code != tc.want || len(db.execs) != 0 {
				t.Fatalf("status=%d (want %d) code=%q writes=%d", recorder.Code, tc.want, typedCode(t, recorder), len(db.execs))
			}
		})
	}
}
