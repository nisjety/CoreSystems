package resources

import (
	"github.com/go-chi/chi/v5"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

func MountBlocklists(r chi.Router, db store.DB) {
	mountSimple[store.BlocklistEntry](r, "/v1/blocklists", db.Blocklists())
}
