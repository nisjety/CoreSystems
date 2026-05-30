// Package jetstream provides declarative, language-agnostic specifications
// for NATS JetStream streams and consumers that back the Model Plane event
// bus. The JSON shape mirrors the Rust mp-events crate byte-for-byte so the
// same fixtures validate on both sides.
package jetstream

import (
	"encoding/json"
	"errors"
)

// Retention policy values.
const (
	RetentionLimits    = "limits"
	RetentionInterest  = "interest"
	RetentionWorkQueue = "workqueue"
)

// Storage backend values.
const (
	StorageFile   = "file"
	StorageMemory = "memory"
)

// Discard policy values.
const (
	DiscardOld = "old"
	DiscardNew = "new"
)

// Ack policy values.
const (
	AckExplicit = "explicit"
	AckAll      = "all"
	AckNone     = "none"
)

// StreamSpec is the declarative specification of a JetStream stream.
type StreamSpec struct {
	Name       string   `json:"name"`
	Subjects   []string `json:"subjects"`
	Retention  string   `json:"retention"`
	Storage    string   `json:"storage"`
	MaxAgeSecs uint64   `json:"max_age_secs"`
	Replicas   uint8    `json:"replicas"`
	Discard    string   `json:"discard"`
}

// Validate returns an error if any required field is missing or zero.
func (s *StreamSpec) Validate() error {
	if s.Name == "" {
		return errors.New("missing required field: name")
	}
	if len(s.Subjects) == 0 {
		return errors.New("missing required field: subjects")
	}
	for _, subj := range s.Subjects {
		if subj == "" {
			return errors.New("missing required field: subjects")
		}
	}
	if s.MaxAgeSecs == 0 {
		return errors.New("missing required field: max_age_secs")
	}
	if s.Replicas == 0 {
		return errors.New("missing required field: replicas")
	}
	return nil
}

// Encode serializes the spec to JSON bytes.
func (s *StreamSpec) Encode() ([]byte, error) {
	return json.Marshal(s)
}

// DecodeStream deserializes a StreamSpec from JSON bytes.
func DecodeStream(data []byte) (*StreamSpec, error) {
	var s StreamSpec
	if err := json.Unmarshal(data, &s); err != nil {
		return nil, err
	}
	return &s, nil
}

// ConsumerSpec is the declarative specification of a JetStream consumer.
type ConsumerSpec struct {
	Durable       string `json:"durable"`
	AckPolicy     string `json:"ack_policy"`
	AckWaitSecs   uint64 `json:"ack_wait_secs"`
	MaxDeliver    uint32 `json:"max_deliver"`
	FilterSubject string `json:"filter_subject"`
}

// Validate returns an error if any required field is missing or zero.
func (c *ConsumerSpec) Validate() error {
	if c.Durable == "" {
		return errors.New("missing required field: durable")
	}
	if c.AckWaitSecs == 0 {
		return errors.New("missing required field: ack_wait_secs")
	}
	if c.MaxDeliver == 0 {
		return errors.New("missing required field: max_deliver")
	}
	if c.FilterSubject == "" {
		return errors.New("missing required field: filter_subject")
	}
	return nil
}

// Encode serializes the spec to JSON bytes.
func (c *ConsumerSpec) Encode() ([]byte, error) {
	return json.Marshal(c)
}

// DecodeConsumer deserializes a ConsumerSpec from JSON bytes.
func DecodeConsumer(data []byte) (*ConsumerSpec, error) {
	var c ConsumerSpec
	if err := json.Unmarshal(data, &c); err != nil {
		return nil, err
	}
	return &c, nil
}
