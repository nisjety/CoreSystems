package resources

import (
	"net/http"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

func MountWebhookDeliveries(r chi.Router, db store.DB) {
	mountSimple[store.WebhookDelivery](r, "/v1/webhook-deliveries", db.WebhookDeliveries())

	r.Post("/v1/webhook-deliveries/{id}/retry", func(w http.ResponseWriter, req *http.Request) {
		id := quarrycontracts.ID(chi.URLParam(req, "id"))
		d, ok := db.WebhookDeliveries().Get(id)
		if !ok {
			httpx.WriteErr(w, req, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": id})
			return
		}
		d.Status = "pending"
		d.Attempt = 0
		d.LastError = ""
		d.NextAttemptAt = time.Now().Unix()
		if err := db.WebhookDeliveries().Update(d); err != nil {
			httpx.WriteErr(w, req, quarrycontracts.CodeInternal, err.Error(), nil)
			return
		}
		httpx.WriteJSON(w, req, http.StatusAccepted, d)
	})
}
