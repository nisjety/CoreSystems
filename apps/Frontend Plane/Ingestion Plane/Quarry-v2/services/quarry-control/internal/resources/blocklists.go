package resources

import (
	"encoding/json"
	"net/http"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// MountBlocklists is the blocklist CRUD mount. WS7.F retired it from the
// router: main mounts MountBlocklistsGone instead. Rollback is one line in
// cmd/control/main.go (call this again); the table and store are untouched.
func MountBlocklists(r chi.Router, db store.DB) {
	mountSimple[store.BlocklistEntry](r, "/v1/blocklists", db.Blocklists())
}

// blocklistsGoneCode is the wire code of the 410 on the retired blocklist
// routes. It is deliberately not a quarrycontracts constant: that set mirrors
// the closed Rust ErrorCode enum, and a new variant there is a cross-crate
// change. The handler writes the typed envelope itself. See docs/RUNBOOKS.md.
const blocklistsGoneCode quarrycontracts.ErrorCode = "GONE"

// MountBlocklistsGone answers every method on /v1/blocklists and below with
// 410 Gone and the typed error envelope, pointing at the org policy API.
func MountBlocklistsGone(r chi.Router) {
	gone := http.HandlerFunc(writeBlocklistsGone)
	sub := chi.NewRouter()
	sub.Handle("/", gone)
	sub.Handle("/*", gone)
	r.Mount("/v1/blocklists", sub)
}

func writeBlocklistsGone(w http.ResponseWriter, r *http.Request) {
	env := quarrycontracts.Err(httpx.RequestIDOf(r.Context()), quarrycontracts.ErrorEnvelope{
		Code:    blocklistsGoneCode,
		Message: "/v1/blocklists is retired; blocklists are replaced by the org web policy",
		Details: map[string]any{
			"successor": "org web-policy API (WS7.H, not yet built)",
			"reference": "docs/POLICY.md",
		},
		Retryable: false,
	})
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusGone)
	_ = json.NewEncoder(w).Encode(env)
}
