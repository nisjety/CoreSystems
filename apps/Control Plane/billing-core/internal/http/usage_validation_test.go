package http

import (
	"bytes"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"
)

func TestRecordUsageRequiresStableEventIdentityAndExplicitTimestamp(t *testing.T) {
	gin.SetMode(gin.TestMode)

	tests := []struct {
		name string
		body string
	}{
		{
			name: "missing event id",
			body: `{"metric":"api_calls","quantity":1,"occurred_at":"2026-07-15T10:00:00Z"}`,
		},
		{
			name: "missing occurred at",
			body: `{"event_id":"usage_01","metric":"api_calls","quantity":1}`,
		},
		{
			name: "invalid occurred at",
			body: `{"event_id":"usage_01","metric":"api_calls","quantity":1,"occurred_at":"yesterday"}`,
		},
		{
			name: "blank event id",
			body: `{"event_id":"   ","metric":"api_calls","quantity":1,"occurred_at":"2026-07-15T10:00:00Z"}`,
		},
		{
			name: "oversized event id",
			body: `{"event_id":"` + strings.Repeat("a", 129) + `","metric":"api_calls","quantity":1,"occurred_at":"2026-07-15T10:00:00Z"}`,
		},
		{
			name: "unsafe event id characters",
			body: `{"event_id":"usage id","metric":"api_calls","quantity":1,"occurred_at":"2026-07-15T10:00:00Z"}`,
		},
		{
			name: "negative quantity",
			body: `{"event_id":"usage_01","metric":"api_calls","quantity":-1,"occurred_at":"2026-07-15T10:00:00Z"}`,
		},
		{
			name: "oversized metric",
			body: `{"event_id":"usage_01","metric":"` + strings.Repeat("m", 129) + `","quantity":1,"occurred_at":"2026-07-15T10:00:00Z"}`,
		},
		{
			name: "oversized metadata",
			body: `{"event_id":"usage_01","metric":"api_calls","quantity":1,"occurred_at":"2026-07-15T10:00:00Z","metadata":{"payload":"` + strings.Repeat("x", 16*1024) + `"}}`,
		},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			server := &Server{}
			router := gin.New()
			router.Use(gin.Recovery())
			router.POST("/orgs/:orgId/usage", server.recordUsage)

			request := httptest.NewRequest(
				http.MethodPost,
				"/orgs/org-usage/usage",
				bytes.NewBufferString(test.body),
			)
			request.Header.Set("Content-Type", "application/json")
			response := httptest.NewRecorder()
			router.ServeHTTP(response, request)

			if response.Code != http.StatusBadRequest {
				t.Fatalf("status=%d body=%s; want 400", response.Code, response.Body.String())
			}
		})
	}
}
