package http

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/users"
)

// WS16-S2 preference contract v2 at the HTTP boundary. The validation tests
// run with no userService at all: every rejection must happen before any
// write. The read/write round trips are gated on TEST_DATABASE_URL and run
// against a throwaway schema holding only user_settings.

func prefsRequest(s *Server, handler func(*Server, *gin.Context), method, body, user string) *httptest.ResponseRecorder {
	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)
	c.Request = httptest.NewRequest(method, "/api/v1/preferences", strings.NewReader(body))
	c.Request.Header.Set("Content-Type", "application/json")
	if user != "" {
		c.Set("user_id", user)
	}
	handler(s, c)
	return w
}

func TestUpdatePreferencesRejectsBadFieldsBeforeAnyWrite(t *testing.T) {
	gin.SetMode(gin.TestMode)
	s := &Server{} // userService is nil: any write attempt would panic.
	cases := map[string]string{
		"bad timezone":         `{"timezone":"US"}`,
		"Local timezone":       `{"timezone":"Local"}`,
		"unknown zone":         `{"timezone":"Mars/Olympus"}`,
		"too long language":    `{"language":"english"}`,
		"underscore language":  `{"language":"en_US"}`,
		"lowercase region":     `{"language":"en-us"}`,
		"bad dateFormat":       `{"dateFormat":"YYYY/MM/DD"}`,
		"bad timeFormat":       `{"timeFormat":"13h"}`,
		"empty dateFormat":     `{"dateFormat":""}`,
		"valid then bad field": `{"theme":"dark","dateFormat":"nope"}`,
	}
	for name, body := range cases {
		t.Run(name, func(t *testing.T) {
			w := prefsRequest(s, (*Server).updatePreferences, http.MethodPatch, body, "11111111-1111-1111-1111-111111111111")
			if w.Code != http.StatusBadRequest {
				t.Fatalf("want 400, got %d (%s)", w.Code, w.Body.String())
			}
			var resp map[string]any
			if err := json.Unmarshal(w.Body.Bytes(), &resp); err != nil {
				t.Fatalf("decode: %v", err)
			}
			msg, isString := resp["error"].(string)
			if !isString || msg == "" {
				t.Fatalf("want the legacy {\"error\": string} body, got %s", w.Body.String())
			}
			if len(resp) != 1 {
				t.Fatalf("legacy body carries only the error key, got %v", resp)
			}
		})
	}
}

func TestUpdatePreferencesRejectsMalformedBodyAndMissingUser(t *testing.T) {
	gin.SetMode(gin.TestMode)
	s := &Server{}
	if w := prefsRequest(s, (*Server).updatePreferences, http.MethodPatch, `{"compactMode":"yes"}`, "u1"); w.Code != http.StatusBadRequest {
		t.Fatalf("non-bool compactMode: want 400, got %d", w.Code)
	}
	if w := prefsRequest(s, (*Server).updatePreferences, http.MethodPatch, `{"theme":"dark"}`, ""); w.Code != http.StatusUnauthorized {
		t.Fatalf("no user in context: want 401, got %d", w.Code)
	}
	if w := prefsRequest(s, (*Server).getPreferences, http.MethodGet, ``, ""); w.Code != http.StatusUnauthorized {
		t.Fatalf("GET without user: want 401, got %d", w.Code)
	}
}

// newPreferencesServer builds a Server over a real users.Service whose
// repository points at an isolated schema containing only user_settings.
func newPreferencesServer(t *testing.T) (*Server, *pgxpool.Pool) {
	t.Helper()
	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("TEST_DATABASE_URL not set; skipping live preferences test")
	}
	ctx := context.Background()
	admin, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	t.Cleanup(admin.Close)
	schema := fmt.Sprintf("ws16s2_%d", time.Now().UnixNano())
	if _, err := admin.Exec(ctx, "CREATE SCHEMA "+schema); err != nil {
		t.Fatalf("create schema: %v", err)
	}
	t.Cleanup(func() { _, _ = admin.Exec(context.Background(), "DROP SCHEMA "+schema+" CASCADE") })
	cfg, err := pgxpool.ParseConfig(dsn)
	if err != nil {
		t.Fatalf("parse dsn: %v", err)
	}
	cfg.ConnConfig.RuntimeParams["search_path"] = schema
	pool, err := pgxpool.NewWithConfig(ctx, cfg)
	if err != nil {
		t.Fatalf("pool: %v", err)
	}
	t.Cleanup(pool.Close)
	if _, err := pool.Exec(ctx, `CREATE TABLE user_settings (
		id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
		user_id UUID NOT NULL,
		category VARCHAR(50) NOT NULL,
		settings JSONB NOT NULL DEFAULT '{}',
		created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
		updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW());
		CREATE UNIQUE INDEX ON user_settings(user_id, category)`); err != nil {
		t.Fatalf("create user_settings: %v", err)
	}
	svc := users.NewService(users.NewRepository(&database.DB{Pool: pool}), nil, nil)
	return &Server{userService: svc}, pool
}

type prefsResponse struct {
	Preferences map[string]any `json:"preferences"`
}

func getPrefs(t *testing.T, s *Server, user string) map[string]any {
	t.Helper()
	w := prefsRequest(s, (*Server).getPreferences, http.MethodGet, ``, user)
	if w.Code != http.StatusOK {
		t.Fatalf("GET: want 200, got %d (%s)", w.Code, w.Body.String())
	}
	var resp prefsResponse
	if err := json.Unmarshal(w.Body.Bytes(), &resp); err != nil {
		t.Fatalf("decode: %v", err)
	}
	return resp.Preferences
}

func TestGetPreferencesNewUserHasNoTimezoneAndNothingExplicitLive(t *testing.T) {
	gin.SetMode(gin.TestMode)
	s, _ := newPreferencesServer(t)
	prefs := getPrefs(t, s, uuid.NewString())
	if _, present := prefs["timezone"]; present {
		t.Fatalf("a new user has no timezone, got %v", prefs["timezone"])
	}
	explicit, _ := prefs["explicit"].(map[string]any)
	if len(explicit) != 5 {
		t.Fatalf("explicit = %v", explicit)
	}
	for k, v := range explicit {
		if v != false {
			t.Fatalf("explicit[%s] = %v for a new user", k, v)
		}
	}
	if prefs["dateFormat"] != "MM/DD/YYYY" || prefs["timeFormat"] != "12h" || prefs["compactMode"] != false {
		t.Fatalf("defaults: %v", prefs)
	}
}

func TestUpdatePreferencesWritesTimezoneKeyAndCompactModeLive(t *testing.T) {
	gin.SetMode(gin.TestMode)
	s, pool := newPreferencesServer(t)
	user := uuid.NewString()
	body := `{"timezone":"Europe/Oslo","language":"nb-NO","dateFormat":"DD/MM/YYYY","timeFormat":"24h","compactMode":true,"theme":"dark"}`
	if w := prefsRequest(s, (*Server).updatePreferences, http.MethodPatch, body, user); w.Code != http.StatusOK {
		t.Fatalf("PATCH: want 200, got %d (%s)", w.Code, w.Body.String())
	}
	ctx := context.Background()
	var lang, appearance string
	if err := pool.QueryRow(ctx, `SELECT settings::text FROM user_settings WHERE user_id=$1 AND category='language'`, user).Scan(&lang); err != nil {
		t.Fatalf("language row: %v", err)
	}
	if !strings.Contains(lang, `"timezone": "Europe/Oslo"`) {
		t.Fatalf("language row must carry the timezone key: %s", lang)
	}
	if strings.Contains(lang, "region") {
		t.Fatalf("the legacy region key must not be written: %s", lang)
	}
	if err := pool.QueryRow(ctx, `SELECT settings::text FROM user_settings WHERE user_id=$1 AND category='appearance'`, user).Scan(&appearance); err != nil {
		t.Fatalf("appearance row: %v", err)
	}
	if !strings.Contains(appearance, `"compactMode": true`) || strings.Contains(lang, "compactMode") {
		t.Fatalf("compactMode belongs to appearance only: appearance=%s language=%s", appearance, lang)
	}
	prefs := getPrefs(t, s, user)
	if prefs["timezone"] != "Europe/Oslo" || prefs["language"] != "nb-NO" || prefs["dateFormat"] != "DD/MM/YYYY" || prefs["timeFormat"] != "24h" || prefs["compactMode"] != true || prefs["theme"] != "dark" {
		t.Fatalf("round trip: %v", prefs)
	}
	explicit, _ := prefs["explicit"].(map[string]any)
	for _, k := range []string{"language", "timezone", "dateFormat", "timeFormat", "compactMode"} {
		if explicit[k] != true {
			t.Fatalf("explicit[%s] = %v after an explicit write", k, explicit[k])
		}
	}
}

func TestUpdatePreferencesAutoResetsExplicitAndLegacyFieldsUnchangedLive(t *testing.T) {
	gin.SetMode(gin.TestMode)
	s, _ := newPreferencesServer(t)
	user := uuid.NewString()
	patch := func(body string) {
		t.Helper()
		if w := prefsRequest(s, (*Server).updatePreferences, http.MethodPatch, body, user); w.Code != http.StatusOK {
			t.Fatalf("PATCH %s: want 200, got %d (%s)", body, w.Code, w.Body.String())
		}
	}
	patch(`{"dateFormat":"YYYY-MM-DD","timeFormat":"24h"}`)
	patch(`{"dateFormat":"auto","timeFormat":"auto"}`)
	prefs := getPrefs(t, s, user)
	explicit, _ := prefs["explicit"].(map[string]any)
	if explicit["dateFormat"] != false || explicit["timeFormat"] != false {
		t.Fatalf("auto must clear explicitness: %v", explicit)
	}
	// Legacy fields (notifications, crawlIngestMode) keep their behaviour.
	patch(`{"notifications":{"emailNotifications":false},"crawlIngestMode":"never"}`)
	prefs = getPrefs(t, s, user)
	notif, _ := prefs["notifications"].(map[string]any)
	if notif["email"] != false || prefs["crawlIngestMode"] != "never" {
		t.Fatalf("legacy fields changed: %v", prefs)
	}
	if w := prefsRequest(s, (*Server).updatePreferences, http.MethodPatch, `{"crawlIngestMode":"sometimes"}`, user); w.Code != http.StatusBadRequest {
		t.Fatalf("bad crawlIngestMode: want 400, got %d", w.Code)
	}
}
