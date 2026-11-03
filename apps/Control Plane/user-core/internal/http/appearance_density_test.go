package http

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"
)

func appearanceDensityContext(method, target, body string) (*gin.Context, *httptest.ResponseRecorder) {
	gin.SetMode(gin.TestMode)
	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)
	c.Request = httptest.NewRequest(method, target, strings.NewReader(body))
	c.Request.Header.Set("Content-Type", "application/json")
	return c, w
}

// A level the person chose is bound into the stored patch, and the stored object reads back with the same level.
func TestAppearanceTranscriptDensityRoundTrip(t *testing.T) {
	c, _ := appearanceDensityContext(http.MethodPut, "/api/v1/settings/appearance",
		`{"theme":"dark","colorScheme":"blue","fontSize":"medium","compactMode":false,"transcriptDensity":"minimal"}`)
	var req AppearanceSettings
	patch, ok := bindSettingsPatch(c, &req)
	if !ok {
		t.Fatal("a valid save was refused")
	}
	if patch["transcriptDensity"] != "minimal" {
		t.Fatalf("patch = %v, want transcriptDensity minimal", patch)
	}
	read, w := appearanceDensityContext(http.MethodGet, "/api/v1/settings/appearance", "")
	writeSettingsResponse[AppearanceSettings](read, patch)
	var got AppearanceSettings
	if err := json.Unmarshal(w.Body.Bytes(), &got); err != nil {
		t.Fatalf("response is not JSON: %v: %s", err, w.Body.String())
	}
	if got.TranscriptDensity != "minimal" {
		t.Fatalf("read back %q, want minimal: %s", got.TranscriptDensity, w.Body.String())
	}
}

// A save that names no level keeps the stored one: the key is left out of the patch. The compact flag is
// written as sent, and the server invents no level from it.
func TestAppearanceCompactModeKeptWhenDensityAbsent(t *testing.T) {
	c, _ := appearanceDensityContext(http.MethodPut, "/api/v1/settings/appearance",
		`{"theme":"dark","colorScheme":"blue","fontSize":"medium","compactMode":true}`)
	var req AppearanceSettings
	patch, ok := bindSettingsPatch(c, &req)
	if !ok {
		t.Fatal("a save without a level was refused")
	}
	if _, present := patch["transcriptDensity"]; present {
		t.Fatalf("an absent level reached the patch: %v", patch)
	}
	if patch["compactMode"] != true {
		t.Fatalf("compactMode = %v, want true: %v", patch["compactMode"], patch)
	}
	if !validTranscriptDensity(req.TranscriptDensity) {
		t.Fatal("an absent level must pass validation")
	}
	read, w := appearanceDensityContext(http.MethodGet, "/api/v1/settings/appearance", "")
	writeSettingsResponse[AppearanceSettings](read, patch)
	if strings.Contains(w.Body.String(), "transcriptDensity") {
		t.Fatalf("the response invents a level: %s", w.Body.String())
	}
	if !strings.Contains(w.Body.String(), `"compactMode":true`) {
		t.Fatalf("compactMode not read back: %s", w.Body.String())
	}
}

// A level outside the closed set is refused with the typed envelope before the store is touched, so no user
// service is needed here. The allowed levels travel under a named key.
func TestAppearanceRejectsUnknownDensity(t *testing.T) {
	c, w := appearanceDensityContext(http.MethodPut, "/api/v1/settings/appearance",
		`{"theme":"dark","colorScheme":"blue","fontSize":"medium","compactMode":false,"transcriptDensity":"huge"}`)
	c.Set("user_id", "user-1")
	(&Server{}).updateAppearanceSettings(c)
	if w.Code != http.StatusUnprocessableEntity {
		t.Fatalf("status = %d, want 422: %s", w.Code, w.Body.String())
	}
	var body struct {
		Error struct {
			Code    string         `json:"code"`
			Message string         `json:"message"`
			Details map[string]any `json:"details"`
		} `json:"error"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &body); err != nil {
		t.Fatalf("body is not JSON: %v: %s", err, w.Body.String())
	}
	if body.Error.Code != "transcript_density_invalid" || body.Error.Message == "" {
		t.Fatalf("envelope = %+v", body.Error)
	}
	if allowed, ok := body.Error.Details["allowed"].([]any); !ok || len(allowed) != 4 {
		t.Fatalf("details carry no allowed list under a named key: %v", body.Error.Details)
	}
}

// The closed set is the four levels, and an empty level is not named at all.
func TestAppearanceTranscriptDensityClosedSet(t *testing.T) {
	for _, level := range []string{"detailed", "standard", "compact", "minimal", ""} {
		if !validTranscriptDensity(level) {
			t.Fatalf("level %q was refused", level)
		}
	}
	for _, level := range []string{"Compact", "full", " minimal", "minimal\n", "none"} {
		if validTranscriptDensity(level) {
			t.Fatalf("level %q was accepted", level)
		}
	}
}
