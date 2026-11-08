package http

import (
	"encoding/json"
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

// The tone is a closed set: every preset is accepted, and any other name is refused, case and
// surrounding space included, so model-gateway only ever sees a name it renders.
func TestAISettingsTonePresetIsClosedSet(t *testing.T) {
	c, w := aiSettingsPutContext(`{"tonePreset":"direct"}`)
	var req AISettings
	patch, ok := bindSettingsPatch(c, &req)
	if !ok {
		t.Fatalf("bindSettingsPatch refused a valid body: %d %s", w.Code, w.Body.String())
	}
	for _, bogus := range []string{"Direct", " direct", "formal", "sarcastic"} {
		req.TonePreset = bogus
		if applyTonePresetPatch(req, patch) {
			t.Fatalf("a save with tone %q was accepted", bogus)
		}
	}
	for _, preset := range tonePresets {
		req.TonePreset = preset
		if !applyTonePresetPatch(req, patch) {
			t.Fatalf("a save with tone %q was refused", preset)
		}
	}
}

// "default" is what a person reads before they save any tone, and it asks for nothing extra.
func TestAISettingsDefaultTonePresetIsDefault(t *testing.T) {
	if got := aiSettingsDefaults()["tonePreset"]; got != "default" {
		t.Fatalf("default tone = %v, want \"default\"", got)
	}
}

// A save that names no tone must not write one, so the stored tone survives every client written
// before tones existed.
func TestAISettingsSaveWithoutToneKeepsTheStoredTone(t *testing.T) {
	c, w := aiSettingsPutContext(`{"aiEnabled":true,"modelPreference":"auto"}`)
	var req AISettings
	patch, ok := bindSettingsPatch(c, &req)
	if !ok {
		t.Fatalf("bindSettingsPatch refused a valid body: %d %s", w.Code, w.Body.String())
	}
	if !applyTonePresetPatch(req, patch) {
		t.Fatal("a save without a tone was refused")
	}
	if _, present := patch["tonePreset"]; present {
		t.Fatalf("a save without a tone wrote one: %v", patch)
	}
}

func TestAISettingsSaveWithANamedToneWritesIt(t *testing.T) {
	c, _ := aiSettingsPutContext(`{"aiEnabled":true,"tonePreset":"warm"}`)
	var req AISettings
	patch, ok := bindSettingsPatch(c, &req)
	if !ok {
		t.Fatal("bindSettingsPatch refused a valid body")
	}
	if !applyTonePresetPatch(req, patch) || patch["tonePreset"] != "warm" {
		t.Fatalf("a named tone was not written: %v", patch)
	}
}

// The selector shows only for the literal true, the strict parser every switch here uses.
func TestTonePresetsVisibleIsStrict(t *testing.T) {
	for value, want := range map[string]bool{"": false, "1": false, "TRUE": false, " true": false, "true": true} {
		t.Setenv("TONE_PRESETS_VISIBLE", value)
		if got := tonePresetsVisible(); got != want {
			t.Fatalf("TONE_PRESETS_VISIBLE=%q: visible = %v, want %v", value, got, want)
		}
	}
}

// The response carries the stored tone and the selector flag, and the flag comes from the switch.
func TestAISettingsResponseCarriesTheToneAndTheSwitch(t *testing.T) {
	t.Setenv("TONE_PRESETS_VISIBLE", "true")
	gin.SetMode(gin.TestMode)
	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)
	writeAISettings(c, map[string]any{"aiEnabled": true, "tonePreset": "warm"})

	var body map[string]any
	if err := json.Unmarshal(w.Body.Bytes(), &body); err != nil {
		t.Fatal(err)
	}
	if body["tonePreset"] != "warm" || body["tonePresetsAvailable"] != true {
		t.Fatalf("response = %v", body)
	}
}

// A stored record with no tone reads as the default, never as an empty name.
func TestAISettingsResponseReadsAMissingToneAsDefault(t *testing.T) {
	gin.SetMode(gin.TestMode)
	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)
	writeAISettings(c, map[string]any{"aiEnabled": true})

	var body map[string]any
	if err := json.Unmarshal(w.Body.Bytes(), &body); err != nil {
		t.Fatal(err)
	}
	if body["tonePreset"] != "default" {
		t.Fatalf("missing tone read as %v, want \"default\"", body["tonePreset"])
	}
}
