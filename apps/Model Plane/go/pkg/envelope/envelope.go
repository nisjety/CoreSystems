// Package envelope provides the canonical event envelope struct for the Model Plane.
package envelope

import (
	"encoding/hex"
	"encoding/json"
	"errors"
	"time"

	"github.com/zeebo/blake3"
)

// DeriveIdempotencyHash computes the blake3 hex digest of
// "producer|event_type|resource_ref|idempotency_key". Must match the Rust
// mp-events implementation byte-for-byte.
func DeriveIdempotencyHash(producer, eventType, resourceRef, idempotencyKey string) string {
	h := blake3.New()
	sep := []byte("|")
	h.Write([]byte(producer))
	h.Write(sep)
	h.Write([]byte(eventType))
	h.Write(sep)
	h.Write([]byte(resourceRef))
	h.Write(sep)
	h.Write([]byte(idempotencyKey))
	return hex.EncodeToString(h.Sum(nil))
}

// Envelope is the canonical event envelope matching the proto Event message.
type Envelope struct {
	EventID        string          `json:"event_id"`
	EventType      string          `json:"event_type"`
	SchemaVersion  uint32          `json:"schema_version"`
	Ts             time.Time       `json:"ts"`
	Producer       string          `json:"producer"`
	CorrelationID  string          `json:"correlation_id"`
	CausationID    string          `json:"causation_id"`
	IdempotencyKey string          `json:"idempotency_key"`
	OrgID          string          `json:"org_id"`
	UserID         string          `json:"user_id"`
	ResourceRef    string          `json:"resource_ref"`
	Payload        json.RawMessage `json:"payload"`

	// Zdr is the Zero Data Retention posture declared for this event: when true
	// the event must not be persisted durably. It is proto field 13
	// (`model_plane.v1.Event.zdr`) and the JSON key `zdr` is the same key the
	// Rust twin serializes (`mp-events/src/envelope.rs`), so both producers
	// agree on the wire.
	//
	// # Why a pointer where the proto and Rust use a plain bool
	//
	// Three states have to survive the wire, not two:
	//
	//	nil    → the producer declared NOTHING. The key is omitted entirely.
	//	&false → the producer declared the event retainable.
	//	&true  → the producer declared Zero Data Retention.
	//
	// A plain bool cannot express the first, and a producer that does not know
	// its run's posture must not be forced to assert `false` — that is exactly
	// how a no-retention run's content silently becomes durable downstream.
	// Consumers are expected to fail closed on nil (see [DeclaredZDR]).
	//
	// `omitempty` on a pointer omits only nil, so an explicit `false` is still
	// emitted as `"zdr": false` — which is what a fail-closed consumer needs in
	// order to permit anything at all.
	Zdr *bool `json:"zdr,omitempty"`
}

// ZDRFlag returns a pointer to zdr, for setting [Envelope.Zdr] inline. Go has
// no address-of-literal, so without this every producer needs a throwaway
// variable to declare a posture.
func ZDRFlag(zdr bool) *bool { return &zdr }

// IsZDR reports whether this envelope declares Zero Data Retention. An
// undeclared posture is NOT Zero Data Retention — it is unknown, and callers
// that need to distinguish the two must use [Envelope.Zdr] or [DeclaredZDR]
// directly. Use this only for the "must this be suppressed" question, where
// suppressing an unknown-posture event would silently drop lifecycle signal.
func (e *Envelope) IsZDR() bool { return e.Zdr != nil && *e.Zdr }

// DeclaredZDR probes raw envelope bytes for the `zdr` flag, keeping an absent
// key distinguishable from an explicit `false`.
//
// declared is false when the bytes are not JSON, are not an object, or carry no
// `zdr` key — every case in which nobody has asserted a retention posture. It
// reads the wire rather than a decoded [Envelope] so it is usable on the
// publish path, where the payload is already serialized, and so that a lossy
// struct on either side cannot launder an absent flag into a `false` one.
func DeclaredZDR(data []byte) (zdr bool, declared bool) {
	var probe struct {
		ZDR *bool `json:"zdr"`
	}
	if err := json.Unmarshal(data, &probe); err != nil || probe.ZDR == nil {
		return false, false
	}
	return *probe.ZDR, true
}

// Validate checks that all required envelope fields are present and non-empty.
func (e *Envelope) Validate() error {
	if e.EventID == "" {
		return errors.New("missing required field: event_id")
	}
	if e.EventType == "" {
		return errors.New("missing required field: event_type")
	}
	if e.SchemaVersion == 0 {
		return errors.New("missing required field: schema_version")
	}
	if e.Producer == "" {
		return errors.New("missing required field: producer")
	}
	if e.OrgID == "" {
		return errors.New("missing required field: org_id")
	}
	if e.Ts.IsZero() {
		return errors.New("missing required field: ts")
	}
	if e.CorrelationID == "" {
		return errors.New("missing required field: correlation_id")
	}
	if e.IdempotencyKey == "" {
		return errors.New("missing required field: idempotency_key")
	}
	if e.UserID == "" {
		return errors.New("missing required field: user_id")
	}
	if e.ResourceRef == "" {
		return errors.New("missing required field: resource_ref")
	}
	return nil
}

// Encode serializes the envelope to JSON bytes.
func (e *Envelope) Encode() ([]byte, error) {
	return json.Marshal(e)
}

// Decode deserializes an envelope from JSON bytes.
func Decode(data []byte) (*Envelope, error) {
	var env Envelope
	if err := json.Unmarshal(data, &env); err != nil {
		return nil, err
	}
	return &env, nil
}
