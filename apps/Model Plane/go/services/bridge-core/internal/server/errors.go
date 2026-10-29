package server

import (
	"errors"
	"net/http"

	"github.com/triodelab/model-plane/services/bridge-core/internal/session"
)

// apiError is the standard JSON error envelope returned by all HTTP endpoints.
type apiError struct {
	Code    int    `json:"code"`
	Message string `json:"message"`
}

// httpStatus maps a domain error to the appropriate HTTP status code. Unknown
// errors default to 500 Internal Server Error.
func httpStatus(err error) int {
	switch {
	case errors.Is(err, session.ErrSessionNotFound):
		return http.StatusNotFound
	case errors.Is(err, session.ErrSessionClosed):
		return http.StatusConflict
	default:
		return http.StatusInternalServerError
	}
}

// errorOutcome classifies an error for telemetry labels.
func errorOutcome(err error) string {
	switch {
	case err == nil:
		return "ok"
	case errors.Is(err, session.ErrSessionNotFound):
		return "not_found"
	case errors.Is(err, session.ErrSessionClosed):
		return "already_closed"
	default:
		return "internal_error"
	}
}
