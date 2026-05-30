package resources

import (
	"github.com/go-chi/chi/v5"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

func MountWebhooks(r chi.Router, db store.DB) {
	mountSimple[store.Webhook](r, "/v1/webhooks", db.Webhooks())
}
