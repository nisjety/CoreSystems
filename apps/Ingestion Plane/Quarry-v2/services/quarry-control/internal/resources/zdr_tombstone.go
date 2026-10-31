// ZDR run tombstone receiver (WS4.J) — POST /v1/internal/zdr-tombstones.
//
// A tombstone is a metadata-only record that a run happened under Zero Data
// Retention (see store/zdr_tombstones.go and migrations/015). This route is a NEW
// EGRESS PATH under ZDR, so it is closed on every side:
//
//   - Authentication: it is mounted inside the hmacVerifier group in
//     cmd/control/main.go (signature, skew and nonce replay come from there), the
//     handler additionally refuses a request that carries no signature header
//     (the verifier lets an unsigned request through during a rollout), and main.go
//     does not mount it at all when the process has no HMAC secret.
//   - Shape: the body is read through a 4 KiB cap into a typed struct with
//     DisallowUnknownFields, so a field outside the allowlist is a 400. The
//     allowlist is crates/quarry-runtime/contracts/zdr-tombstone.schema.json; a
//     test compares this struct's keys, enums and required set to the schema.
//   - Values: every string anywhere in the body (keys included) that contains
//     "://" is a 400 before anything else is looked at, then store.ZdrTombstone
//     .Validate checks ids against a pattern and classes against closed lists.
//   - Answers: 204 for an insert and for a duplicate run_id alike (first write
//     wins), so the route is not an existence oracle. Error bodies name a field,
//     never echo a value.
//   - There is NO HTTP route that reads tombstones; ListByOrg is store-only.
//
// The body must not use the key "zdr": change_webhook.go rejects a truthy "zdr"
// and that gate stays; a tombstone says retention_class "zdr" instead.
//
// OFF BY DEFAULT: the edge emits nothing unless QUARRY_EDGE__ZDR_TOMBSTONE is on,
// and counsel review gates turning it on.
package resources

import (
	"bytes"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"strings"
	"time"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// maxZdrTombstoneBytes caps the request body. A tombstone is a few hundred bytes.
const maxZdrTombstoneBytes = 4 << 10

// ZdrTombstoneWire is the request body: exactly the keys of
// zdr-tombstone.schema.json. Adding a field here without the schema (or the
// reverse) fails TestZdrTombstoneStructMatchesSchema.
type ZdrTombstoneWire struct {
	RunID          string   `json:"run_id"`
	OrgID          string   `json:"org_id"`
	Kind           string   `json:"kind"`
	RetentionClass string   `json:"retention_class"`
	Outcome        string   `json:"outcome"`
	Driver         *string  `json:"driver,omitempty"`
	StartedAt      *string  `json:"started_at,omitempty"`
	EndedAt        string   `json:"ended_at"`
	StepCount      *int     `json:"step_count,omitempty"`
	CostUnits      *float64 `json:"cost_units,omitempty"`
	ErrorClass     *string  `json:"error_class,omitempty"`
}

// MountZdrTombstones registers POST /v1/internal/zdr-tombstones. Call it from
// inside the HMAC-signed route group, and only when the HMAC verifier has a secret.
func MountZdrTombstones(r chi.Router, db store.DB) {
	r.Post("/v1/internal/zdr-tombstones", zdrTombstoneHandler(db))
}

func zdrTombstoneHandler(db store.DB) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		// The verifier accepts an unsigned request while it is in rollout mode;
		// this route never does. A signature that is present was verified by the
		// group middleware before this handler ran.
		if r.Header.Get(httpx.HMACHeaderSig) == "" {
			httpx.WriteErr(w, r, quarrycontracts.CodeUnauthorized, "signed request required", nil)
			return
		}

		r.Body = http.MaxBytesReader(w, r.Body, maxZdrTombstoneBytes)
		raw, err := io.ReadAll(r.Body)
		if err != nil {
			var tooBig *http.MaxBytesError
			if errors.As(err, &tooBig) {
				httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "body too large", nil)
				return
			}
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "unreadable body", nil)
			return
		}

		// First guard: no string anywhere in the body, key or value, may look
		// like a URL. Checked before the typed decode so that no later change to
		// a field's type or vocabulary can let one through.
		if looksLikeURL(raw) {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "a value looks like a URL; tombstones carry no URLs", nil)
			return
		}

		var wire ZdrTombstoneWire
		dec := json.NewDecoder(bytes.NewReader(raw))
		dec.DisallowUnknownFields()
		if err := dec.Decode(&wire); err != nil {
			// The decoder's message can quote the offending key; keep it out.
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "body is not an allowlisted tombstone", nil)
			return
		}
		if _, err := dec.Token(); !errors.Is(err, io.EOF) {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "body is not an allowlisted tombstone", nil)
			return
		}

		t, err := wire.toStore()
		if err == nil {
			err = t.Validate()
		}
		if err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}

		if _, err := db.ZdrTombstones().Put(t); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeInternal, "tombstone not stored", nil)
			return
		}
		// 204 whether this inserted or the run id already had a row: first write
		// wins, and the answer tells the caller nothing about what exists.
		w.WriteHeader(http.StatusNoContent)
	}
}

// looksLikeURL reports whether any JSON string in raw (an object key or a value
// at any depth) contains "://". A body that is not JSON is left to the typed
// decode to refuse, but a raw "://" anywhere in it is refused here as well.
func looksLikeURL(raw []byte) bool {
	if bytes.Contains(raw, []byte("://")) {
		return true
	}
	// A JSON string may spell the slash as an escape sequence; decode and look.
	var v any
	if err := json.Unmarshal(raw, &v); err != nil {
		return false
	}
	return anyStringHasScheme(v)
}

func anyStringHasScheme(v any) bool {
	switch x := v.(type) {
	case string:
		return strings.Contains(x, "://")
	case []any:
		for _, item := range x {
			if anyStringHasScheme(item) {
				return true
			}
		}
	case map[string]any:
		for k, item := range x {
			if strings.Contains(k, "://") || anyStringHasScheme(item) {
				return true
			}
		}
	}
	return false
}

var errZdrTombstoneTime = errors.New("zdr tombstone: value outside the allowlist: timestamp")

func (w ZdrTombstoneWire) toStore() (store.ZdrTombstone, error) {
	ended, err := time.Parse(time.RFC3339, w.EndedAt)
	if err != nil {
		return store.ZdrTombstone{}, errZdrTombstoneTime
	}
	t := store.ZdrTombstone{
		RunID:          w.RunID,
		OrgID:          w.OrgID,
		Kind:           w.Kind,
		RetentionClass: w.RetentionClass,
		Outcome:        w.Outcome,
		Driver:         w.Driver,
		EndedAt:        ended.UTC(),
		StepCount:      w.StepCount,
		CostUnits:      w.CostUnits,
		ErrorClass:     w.ErrorClass,
	}
	if w.StartedAt != nil {
		started, err := time.Parse(time.RFC3339, *w.StartedAt)
		if err != nil {
			return store.ZdrTombstone{}, errZdrTombstoneTime
		}
		started = started.UTC()
		t.StartedAt = &started
	}
	return t, nil
}
