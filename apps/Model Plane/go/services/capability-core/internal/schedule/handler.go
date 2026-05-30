package schedule

import (
	"encoding/json"
	"net/http"
)

// NewHandler returns an HTTP handler that serves the seeded scheduled-work
// catalog as JSON. Only GET is supported; any other method returns 405.
func NewHandler() http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodGet {
			w.Header().Set("Allow", http.MethodGet)
			http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(Load())
	})
}
