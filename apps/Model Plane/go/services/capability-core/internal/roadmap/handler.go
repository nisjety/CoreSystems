package roadmap

import (
	"encoding/json"
	"net/http"
)

// NewHandler returns an http.Handler serving the implementation-status catalog.
// Only GET is permitted; other methods receive 405 with an Allow: GET header.
func NewHandler() http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodGet {
			w.Header().Set("Allow", http.MethodGet)
			http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		_ = json.NewEncoder(w).Encode(Load())
	})
}
