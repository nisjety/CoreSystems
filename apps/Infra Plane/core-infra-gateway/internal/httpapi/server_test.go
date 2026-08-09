package httpapi

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestHandlerExposesOnlyLocalInfrastructureState(t *testing.T) {
	handler := NewHandler(Config{
		ServiceName:   "core-infra-gateway",
		Version:       "test",
		OperatorToken: "operator-token",
	})

	tests := []struct {
		name       string
		path       string
		token      string
		wantStatus int
		wantRole   string
	}{
		{
			name:       "health is public",
			path:       "/health",
			wantStatus: http.StatusOK,
			wantRole:   "local_traffic_operator",
		},
		{
			name:       "operator status rejects an absent token",
			path:       "/v1/operator/status",
			wantStatus: http.StatusUnauthorized,
		},
		{
			name:       "operator status rejects a wrong token",
			path:       "/v1/operator/status",
			token:      "wrong-token",
			wantStatus: http.StatusUnauthorized,
		},
		{
			name:       "operator status reports bounded scope",
			path:       "/v1/operator/status",
			token:      "operator-token",
			wantStatus: http.StatusOK,
			wantRole:   "local_traffic_operator",
		},
		{
			name:       "legacy database route is not exposed",
			path:       "/v1/database/status",
			wantStatus: http.StatusNotFound,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			req := httptest.NewRequest(http.MethodGet, tt.path, nil)
			if tt.token != "" {
				req.Header.Set("Authorization", "Bearer "+tt.token)
			}
			response := httptest.NewRecorder()

			handler.ServeHTTP(response, req)

			if response.Code != tt.wantStatus {
				t.Fatalf("status = %d, want %d", response.Code, tt.wantStatus)
			}

			if tt.wantRole == "" {
				return
			}

			var body struct {
				Data struct {
					Role string `json:"role"`
				} `json:"data"`
			}
			if err := json.NewDecoder(response.Body).Decode(&body); err != nil {
				t.Fatalf("decode response: %v", err)
			}
			if body.Data.Role != tt.wantRole {
				t.Fatalf("role = %q, want %q", body.Data.Role, tt.wantRole)
			}
		})
	}
}

func TestHandlerDisablesOperatorStatusWithoutConfiguredToken(t *testing.T) {
	handler := NewHandler(Config{ServiceName: "core-infra-gateway", Version: "test"})
	request := httptest.NewRequest(http.MethodGet, "/v1/operator/status", nil)
	request.Header.Set("Authorization", "Bearer an-unconfigured-token")
	response := httptest.NewRecorder()

	handler.ServeHTTP(response, request)

	if response.Code != http.StatusUnauthorized {
		t.Fatalf("status = %d, want %d", response.Code, http.StatusUnauthorized)
	}
}
