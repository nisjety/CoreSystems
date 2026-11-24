package http

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"
	"github.com/google/uuid"
)

// B-REC-06: the legacy PUT /api/v1/settings/language at the HTTP boundary. Rejections run with no userService, so
// any write attempt would panic; the read-back round trip is gated on TEST_DATABASE_URL like its neighbours.

func languageRequest(s *Server, handler func(*Server, *gin.Context), method, body, user string) *httptest.ResponseRecorder {
	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)
	c.Request = httptest.NewRequest(method, "/api/v1/settings/language", strings.NewReader(body))
	c.Request.Header.Set("Content-Type", "application/json")
	if user != "" {
		c.Set("user_id", user)
	}
	handler(s, c)
	return w
}

func TestUpdateLanguageSettingsRejectsBadFieldsBeforeAnyWrite(t *testing.T) {
	gin.SetMode(gin.TestMode)
	s := &Server{}
	cases := map[string]string{
		"junk language":     `{"language":"not a language!"}`,
		"unknown region":    `{"region":"Mars/Olympus"}`,
		"bad dateFormat":    `{"dateFormat":"YYYY/DD/MM"}`,
		"bad timeFormat":    `{"timeFormat":"13h"}`,
		"valid then broken": `{"language":"nb-NO","timeFormat":"nope"}`,
	}
	for name, body := range cases {
		t.Run(name, func(t *testing.T) {
			w := languageRequest(s, (*Server).updateLanguageSettings, http.MethodPut, body, "11111111-1111-1111-1111-111111111111")
			if w.Code != http.StatusBadRequest {
				t.Fatalf("want 400, got %d (%s)", w.Code, w.Body.String())
			}
			var resp map[string]any
			if err := json.Unmarshal(w.Body.Bytes(), &resp); err != nil {
				t.Fatalf("decode: %v", err)
			}
			if msg, isString := resp["error"].(string); !isString || msg == "" {
				t.Fatalf("want the legacy {\"error\": string} body, got %s", w.Body.String())
			}
		})
	}
}

func TestUpdateLanguageSettingsRequiresUser(t *testing.T) {
	gin.SetMode(gin.TestMode)
	if w := languageRequest(&Server{}, (*Server).updateLanguageSettings, http.MethodPut, `{"language":"nb-NO"}`, ""); w.Code != http.StatusUnauthorized {
		t.Fatalf("no user in context: want 401, got %d", w.Code)
	}
}

// A client that reads the default "auto" formats back and sends them again gets 200, and only the sent fields land.
func TestUpdateLanguageSettingsAcceptsAutoReadBackLive(t *testing.T) {
	gin.SetMode(gin.TestMode)
	s, pool := newPreferencesServer(t)
	user := uuid.NewString()
	seed := `{"dateFormat":"auto","timeFormat":"auto"}`
	if w := prefsRequest(s, (*Server).updatePreferences, http.MethodPatch, seed, user); w.Code != http.StatusOK {
		t.Fatalf("seed PATCH: want 200, got %d (%s)", w.Code, w.Body.String())
	}
	read := languageRequest(s, (*Server).getLanguageSettings, http.MethodGet, ``, user)
	if read.Code != http.StatusOK {
		t.Fatalf("legacy GET: want 200, got %d (%s)", read.Code, read.Body.String())
	}
	var back struct {
		DateFormat string `json:"dateFormat"`
		TimeFormat string `json:"timeFormat"`
	}
	if err := json.Unmarshal(read.Body.Bytes(), &back); err != nil {
		t.Fatalf("decode: %v", err)
	}
	resend, _ := json.Marshal(map[string]string{"dateFormat": back.DateFormat, "timeFormat": back.TimeFormat})
	if w := languageRequest(s, (*Server).updateLanguageSettings, http.MethodPut, string(resend), user); w.Code != http.StatusOK {
		t.Fatalf("legacy PUT of the read-back body: want 200, got %d (%s)", w.Code, w.Body.String())
	}
	var lang string
	if err := pool.QueryRow(context.Background(), `SELECT settings::text FROM user_settings WHERE user_id=$1 AND category='language'`, user).Scan(&lang); err != nil {
		t.Fatalf("language row: %v", err)
	}
	if strings.Contains(lang, `"language": ""`) || strings.Contains(lang, `"region": ""`) {
		t.Fatalf("fields the body left out must not be blanked: %s", lang)
	}
}
