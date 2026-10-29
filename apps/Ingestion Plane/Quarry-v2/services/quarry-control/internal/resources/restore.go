package resources

import (
	"encoding/json"
	"net/http"
	"time"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// MountRestore mounts POST /v1/restore which materializes a stored snapshot
// into a new resource (currently: browser profiles).
//
// Contract §1.5: POST /v1/restore { snapshot_id, target, name? }
//   - target="profile": creates a BrowserProfile whose SnapshotURI points at
//     the snapshot's S3 location, allowing future runs to resume from it.
func MountRestore(r chi.Router, db store.DB) {
	r.Post("/v1/restore", restoreHandler(db))
}

type restoreInput struct {
	SnapshotID quarrycontracts.ID `json:"snapshot_id"`
	Target     string             `json:"target"`
	Name       string             `json:"name,omitempty"`
}

func restoreHandler(db store.DB) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		var in restoreInput
		if err := json.NewDecoder(r.Body).Decode(&in); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		if err := in.SnapshotID.MustKind(quarrycontracts.KindSnapshot); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		if in.Target != "profile" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "unsupported target", map[string]any{"target": in.Target})
			return
		}

		snap, ok := db.Snapshots().Get(in.SnapshotID)
		if !ok {
			httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "snapshot not found", map[string]any{"id": in.SnapshotID})
			return
		}

		name := in.Name
		if name == "" {
			name = "restored-" + string(snap.ID)
		}
		profile := store.BrowserProfile{
			ID:          quarrycontracts.NewID(quarrycontracts.KindProfile),
			Name:        name,
			SnapshotURI: snap.Bucket,
			CreatedAt:   time.Now().UnixMilli(),
		}
		if err := db.Profiles().Create(profile); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeConflict, err.Error(), nil)
			return
		}
		httpx.WriteJSON(w, r, http.StatusCreated, profile)
	}
}
