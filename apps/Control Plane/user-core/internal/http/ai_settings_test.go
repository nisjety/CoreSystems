package http

import (
	"net/http"
	"net/http/httptest"
	"os"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"
)

func aiSettingsPutContext(body string) (*gin.Context, *httptest.ResponseRecorder) {
	gin.SetMode(gin.TestMode)
	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)
	c.Request = httptest.NewRequest(http.MethodPut, "/api/v1/settings/ai", strings.NewReader(body))
	c.Request.Header.Set("Content-Type", "application/json")
	return c, w
}

// The PUT is a struct patch: every field of AISettings is written back, zero
// value included. A memoryEnabled field is what once stored a meaningless
// `false` on any save, so the patch must carry no memory key at all.
func TestAISettingsPatchStoresNoMemoryKey(t *testing.T) {
	c, w := aiSettingsPutContext(`{"aiEnabled":true,"modelPreference":"auto"}`)

	var req AISettings
	patch, ok := bindSettingsPatch(c, &req)
	if !ok {
		t.Fatalf("bindSettingsPatch refused a valid body: %d %s", w.Code, w.Body.String())
	}
	for key := range patch {
		if strings.Contains(strings.ToLower(key), "memory") {
			t.Fatalf("AI settings patch carries %q; the retired memoryEnabled must not be stored", key)
		}
	}
	for _, key := range []string{"aiEnabled", "modelPreference", "dataCollectionEnabled", "personalizationEnabled"} {
		if _, present := patch[key]; !present {
			t.Fatalf("AI settings patch lost %q: %v", key, patch)
		}
	}
}

// Old clients still send memoryEnabled. The binder tolerates unknown fields,
// so the request succeeds and the value is dropped rather than stored.
func TestAISettingsPatchToleratesAndDropsLegacyMemoryEnabled(t *testing.T) {
	c, w := aiSettingsPutContext(`{"aiEnabled":false,"memoryEnabled":true}`)

	var req AISettings
	patch, ok := bindSettingsPatch(c, &req)
	if !ok {
		t.Fatalf("legacy body was refused: %d %s", w.Code, w.Body.String())
	}
	if _, present := patch["memoryEnabled"]; present {
		t.Fatalf("legacy memoryEnabled reached the stored patch: %v", patch)
	}
	if patch["aiEnabled"] != false {
		t.Fatalf("aiEnabled = %v, want false", patch["aiEnabled"])
	}
}

// The migration removes the key from rows written before this change, scoped to
// the ai category, and states no backfill.
func TestRetireAIMemoryEnabledMigrationTargetsOnlyTheAICategory(t *testing.T) {
	raw, err := os.ReadFile("../../migrations/030_retire_ai_memory_enabled.up.sql")
	if err != nil {
		t.Fatal(err)
	}
	sql := strings.Join(strings.Fields(string(raw)), " ")
	for _, fragment := range []string{
		"UPDATE user_settings",
		"SET settings = settings - 'memoryEnabled'",
		"category = 'ai'",
	} {
		if !strings.Contains(sql, fragment) {
			t.Fatalf("migration does not contain %q", fragment)
		}
	}
}
