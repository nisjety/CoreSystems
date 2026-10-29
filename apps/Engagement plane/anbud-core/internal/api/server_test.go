package api

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/anbud/internal/secret"
)

const serverKey = "internal-api-key-value"

type fakePinger struct{ err error }

func (f fakePinger) Ping(context.Context) error { return f.err }

func doffinStatus(configured bool) func() []SourceStatus {
	return func() []SourceStatus {
		return []SourceStatus{{
			ID:         "doffin",
			Configured: configured,
			Budgets:    map[string]int{"search": 24, "download": 8},
		}}
	}
}

func newServer(cfg ServerConfig) *fiber.App {
	if cfg.APIKey == "" {
		cfg.APIKey = secret.Value(serverKey)
	}
	if cfg.Sources == nil {
		cfg.Sources = doffinStatus(false)
	}
	return NewServer(cfg)
}

func get(t *testing.T, app *fiber.App, path string, headers map[string]string) (int, map[string]any, string) {
	t.Helper()
	req := httptest.NewRequest(http.MethodGet, path, nil)
	for k, v := range headers {
		req.Header.Set(k, v)
	}
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	defer resp.Body.Close()
	raw, _ := io.ReadAll(resp.Body)
	var decoded map[string]any
	if err := json.Unmarshal(raw, &decoded); err != nil {
		t.Fatalf("%s did not return a JSON object: %q", path, raw)
	}
	return resp.StatusCode, decoded, string(raw)
}

func TestHealthNeedsNoAuthentication(t *testing.T) {
	t.Parallel()

	status, body, _ := get(t, newServer(ServerConfig{}), "/health", nil)
	if status != 200 || body["status"] != "ok" || body["service"] != "anbud-core" {
		t.Fatalf("health = %d %v", status, body)
	}
}

func TestReadyReportsDatabaseAndSources(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name       string
		cfg        ServerConfig
		wantStatus int
		wantTop    string
		wantDB     string
		wantDoffin string
	}{
		{"database up, doffin configured", ServerConfig{DB: fakePinger{}, Sources: doffinStatus(true)}, 200, "ok", "ok", "configured"},
		{"database up, doffin not configured stays ready", ServerConfig{DB: fakePinger{}, Sources: doffinStatus(false)}, 200, "ok", "ok", "not_configured"},
		{"database down", ServerConfig{DB: fakePinger{err: errors.New(`password authentication failed for user "ingestion_user"`)}}, 503, "degraded", "down", "not_configured"},
		{"no database wired", ServerConfig{}, 200, "ok", "skipped", "not_configured"},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			status, body, raw := get(t, newServer(tc.cfg), "/ready", nil)
			if status != tc.wantStatus || body["status"] != tc.wantTop {
				t.Fatalf("ready = %d %v", status, body)
			}
			checks, _ := body["checks"].(map[string]any)
			if checks["db"] != tc.wantDB {
				t.Errorf("checks.db = %v, want %s", checks["db"], tc.wantDB)
			}
			sources, _ := body["sources"].(map[string]any)
			if sources["doffin"] != tc.wantDoffin {
				t.Errorf("sources.doffin = %v, want %s", sources["doffin"], tc.wantDoffin)
			}
			if strings.Contains(raw, "password") || strings.Contains(raw, "ingestion_user") {
				t.Errorf("readiness leaked the database error: %s", raw)
			}
		})
	}
}

func TestSourcesRequiresAuthentication(t *testing.T) {
	t.Parallel()

	app := newServer(ServerConfig{})
	for name, headers := range map[string]map[string]string{
		"no key":    nil,
		"wrong key": {"X-API-Key": "nope"},
	} {
		status, body, _ := get(t, app, "/api/v1/sources", headers)
		if status != 401 {
			t.Fatalf("%s: status = %d", name, status)
		}
		errObj, _ := body["error"].(map[string]any)
		if errObj["code"] != "unauthorized" {
			t.Fatalf("%s: body = %v", name, body)
		}
	}
}

func TestSourcesReportsBudgetsAndNeverSecrets(t *testing.T) {
	t.Parallel()

	app := newServer(ServerConfig{Sources: doffinStatus(true)})
	status, body, raw := get(t, app, "/api/v1/sources", map[string]string{"X-API-Key": serverKey})
	if status != 200 {
		t.Fatalf("status = %d (%s)", status, raw)
	}

	data, _ := body["data"].(map[string]any)
	sources, _ := data["sources"].([]any)
	if len(sources) != 1 {
		t.Fatalf("sources = %v", data)
	}
	first, _ := sources[0].(map[string]any)
	if first["id"] != "doffin" || first["configured"] != true {
		t.Fatalf("source = %v", first)
	}
	budgets, _ := first["budgets_per_minute"].(map[string]any)
	if budgets["search"] != float64(24) || budgets["download"] != float64(8) {
		t.Fatalf("budgets = %v", budgets)
	}
	if strings.Contains(raw, serverKey) {
		t.Fatalf("response leaked a secret: %s", raw)
	}
}

func TestUnknownRoutesUseTheErrorEnvelope(t *testing.T) {
	t.Parallel()

	status, body, _ := get(t, newServer(ServerConfig{}), "/nope", nil)
	errObj, _ := body["error"].(map[string]any)
	if status != 404 || errObj["code"] != "not_found" {
		t.Fatalf("404 = %d %v", status, body)
	}
}
