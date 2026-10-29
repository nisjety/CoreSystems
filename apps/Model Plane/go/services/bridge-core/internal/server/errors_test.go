package server

import (
	"errors"
	"net/http"
	"testing"

	"github.com/triodelab/model-plane/services/bridge-core/internal/session"
)

func TestErrorMappings(t *testing.T) {
	for _, tc := range []struct {
		err     error
		status  int
		outcome string
	}{
		{nil, http.StatusInternalServerError, "ok"},
		{session.ErrSessionNotFound, http.StatusNotFound, "not_found"},
		{session.ErrSessionClosed, http.StatusConflict, "already_closed"},
		{errors.New("x"), http.StatusInternalServerError, "internal_error"},
	} {
		if got := httpStatus(tc.err); got != tc.status {
			t.Fatalf("httpStatus(%v)=%d", tc.err, got)
		}
		if got := errorOutcome(tc.err); got != tc.outcome {
			t.Fatalf("errorOutcome(%v)=%q", tc.err, got)
		}
	}
}
