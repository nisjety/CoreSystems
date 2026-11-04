package http

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/gin-gonic/gin"
)

func onboardingTestContext(method, target, body string) (*gin.Context, *httptest.ResponseRecorder) {
	gin.SetMode(gin.TestMode)
	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)
	c.Request = httptest.NewRequest(method, target, strings.NewReader(body))
	c.Request.Header.Set("Content-Type", "application/json")
	return c, w
}

// The first-run state takes three values and no others, so a client cannot park the step in a state
// no build knows how to show.
func TestOnboardingCategoryClosedValues(t *testing.T) {
	now := time.Unix(0, 0)
	for _, step := range []string{"pending", "completed", "skipped"} {
		if _, ok := onboardingStepPatch(step, now); !ok {
			t.Fatalf("closed value %q was refused", step)
		}
	}
	for _, step := range []string{"", "done", "Completed", "skip", " completed", "pending\n"} {
		if _, ok := onboardingStepPatch(step, now); ok {
			t.Fatalf("open value %q was accepted", step)
		}
	}
}

// The version and the time belong to the server. The patch carries the step the person chose and
// nothing else the client sent.
func TestOnboardingPatchStampsVersionAndTime(t *testing.T) {
	patch, ok := onboardingStepPatch("skipped", time.Date(2026, 11, 3, 10, 30, 0, 0, time.UTC))
	if !ok {
		t.Fatal("skipped was refused")
	}
	if patch["personalStep"] != "skipped" || patch["version"] != onboardingVersion || patch["updatedAt"] != "2026-11-03T10:30:00Z" {
		t.Fatalf("patch = %v", patch)
	}
	if len(patch) != 3 {
		t.Fatalf("patch carries %d keys, want exactly personalStep, version and updatedAt: %v", len(patch), patch)
	}
}

// A person who has never answered reads pending at version one, so the card shows once with no stored row.
func TestOnboardingDefaultsArePendingAtVersionOne(t *testing.T) {
	defaults := onboardingSettingsDefaults()
	if defaults["personalStep"] != personalStepPending || defaults["version"] != onboardingVersion {
		t.Fatalf("defaults = %v", defaults)
	}
}

// A stored step outside the closed set reads as pending, never as completed: an unknown value must not
// hide the card for good.
func TestOnboardingReadNormalisesUnknownStepToPending(t *testing.T) {
	c, w := onboardingTestContext(http.MethodGet, "/api/v1/settings/onboarding", "")
	writeOnboardingSettings(c, map[string]any{"personalStep": "archived", "version": 1.0, "updatedAt": "2026-11-03T10:30:00Z"})
	var got OnboardingSettings
	if err := json.Unmarshal(w.Body.Bytes(), &got); err != nil {
		t.Fatalf("response is not JSON: %v: %s", err, w.Body.String())
	}
	if got.PersonalStep != personalStepPending {
		t.Fatalf("personalStep = %q, want pending", got.PersonalStep)
	}
}

// A PUT naming a step outside the closed set is refused with the typed envelope, its details an object
// that carries the allowed values under a named key. Validation runs before the store is touched, so no
// user service is needed here.
func TestUpdateOnboardingRefusesOpenStepWithTypedEnvelope(t *testing.T) {
	c, w := onboardingTestContext(http.MethodPut, "/api/v1/settings/onboarding", `{"personalStep":"done"}`)
	c.Set("user_id", "user-1")
	(&Server{}).updateOnboardingSettings(c)
	if w.Code != http.StatusBadRequest {
		t.Fatalf("status = %d, want 400: %s", w.Code, w.Body.String())
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
	if body.Error.Code != "personal_step_invalid" || body.Error.Message == "" {
		t.Fatalf("envelope = %+v", body.Error)
	}
	if _, ok := body.Error.Details["allowed"].([]any); !ok {
		t.Fatalf("details carry no allowed list under a named key: %v", body.Error.Details)
	}
}

// PERSONAL_SETUP_VISIBLE is opt-in: only the literal true shows the card.
func TestPersonalSetupVisibleIsOptIn(t *testing.T) {
	for value, want := range map[string]bool{"true": true, "TRUE": false, "1": false, "yes": false, "": false} {
		t.Setenv("PERSONAL_SETUP_VISIBLE", value)
		if got := personalSetupVisible(); got != want {
			t.Fatalf("PERSONAL_SETUP_VISIBLE=%q: visible = %v, want %v", value, got, want)
		}
	}
}

// The AI settings response says whether the card is on, beside the tone flag. It is a response field
// only: no stored AI setting carries it.
func TestAISettingsResponseCarriesPersonalSetupFlag(t *testing.T) {
	for value, want := range map[string]string{"true": `"personalSetupAvailable":true`, "": `"personalSetupAvailable":false`} {
		t.Setenv("PERSONAL_SETUP_VISIBLE", value)
		c, w := onboardingTestContext(http.MethodGet, "/api/v1/settings/ai", "")
		writeAISettings(c, aiSettingsDefaults())
		if !strings.Contains(w.Body.String(), want) {
			t.Fatalf("PERSONAL_SETUP_VISIBLE=%q: response %s lacks %s", value, w.Body.String(), want)
		}
	}
}

// The AI patch cannot store the first-run flag: the binder keeps only the keys AISettings names.
func TestAISettingsPatchNeverStoresPersonalSetupFlag(t *testing.T) {
	c, w := aiSettingsPutContext(`{"aiEnabled":true,"personalSetupAvailable":true}`)
	var req AISettings
	patch, ok := bindSettingsPatch(c, &req)
	if !ok {
		t.Fatalf("body was refused: %d %s", w.Code, w.Body.String())
	}
	if _, present := patch["personalSetupAvailable"]; present {
		t.Fatalf("the first-run flag reached the stored patch: %v", patch)
	}
}
