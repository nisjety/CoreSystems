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
