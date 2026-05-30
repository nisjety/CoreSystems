// Package server implements the task-core HTTP API.
package server

import (
	"encoding/json"
	"errors"
	"net/http"

	"github.com/triodelab/model-plane/services/task-core/internal/store"
)

// APIError is the standard JSON error envelope returned by all endpoints.
type APIError struct {
	Code    int    `json:"code"`
	Message string `json:"message"`
}

// Sentinel API errors used by the server layer.
var (
	ErrMissingOrgID = errors.New("query parameter org_id is required")
	ErrMissingID    = errors.New("task id is required in path")
	ErrInvalidJSON  = errors.New("request body is not valid JSON")
)

// writeError writes a JSON error response with the appropriate HTTP status.
func writeError(w http.ResponseWriter, code int, err error) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(code)
	_ = json.NewEncoder(w).Encode(APIError{
		Code:    code,
		Message: err.Error(),
	})
}

// mapHTTPStatus translates internal store errors to HTTP status codes.
func mapHTTPStatus(err error) int {
	switch {
	case errors.Is(err, store.ErrTaskNotFound):
		return http.StatusNotFound
	case errors.Is(err, store.ErrEmptyName), errors.Is(err, store.ErrEmptyOrgID):
		return http.StatusBadRequest
	default:
		return http.StatusInternalServerError
	}
}
