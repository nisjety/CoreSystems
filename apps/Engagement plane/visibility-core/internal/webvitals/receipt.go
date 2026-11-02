package webvitals

import (
	"bytes"
	"encoding/json"
	"io"
)

// DecodeReceipt rejects ambiguous duplicate keys and unexpected provider fields
// before any receipt is permitted into the durable measurement store.
func DecodeReceipt(raw []byte) (Export, error) {
	if len(raw) > maxBytes || !uniqueJSON(raw) || !receiptShape(raw) {
		return Export{}, ErrInvalid
	}
	var envelope struct {
		Success bool   `json:"success"`
		Data    Export `json:"data"`
	}
	dec := json.NewDecoder(bytes.NewReader(raw))
	dec.DisallowUnknownFields()
	if dec.Decode(&envelope) != nil || dec.Decode(new(any)) != io.EOF || !envelope.Success || !validExport(envelope.Data) {
		return Export{}, ErrInvalid
	}
	return envelope.Data, nil
}
func receiptShape(raw []byte) bool {
	var outer map[string]json.RawMessage
	if json.Unmarshal(raw, &outer) != nil || len(outer) != 2 || outer["success"] == nil || outer["data"] == nil {
		return false
	}
	var data map[string]json.RawMessage
	if json.Unmarshal(outer["data"], &data) != nil {
		return false
	}
	for _, field := range []string{"websiteId", "site", "websiteRevision", "environment", "status", "samples", "observedAt"} {
		if data[field] == nil || bytes.Equal(bytes.TrimSpace(data[field]), []byte("null")) {
			return false
		}
	}
	var samples []map[string]json.RawMessage
	if json.Unmarshal(data["samples"], &samples) != nil || samples == nil {
		return false
	}
	for _, sample := range samples {
		if len(sample) != 7 {
			return false
		}
		for _, field := range []string{"sampleId", "sequence", "metric", "value", "viewport", "path", "receivedAt"} {
			if sample[field] == nil || bytes.Equal(bytes.TrimSpace(sample[field]), []byte("null")) {
				return false
			}
		}
	}
	return true
}
func uniqueJSON(raw []byte) bool {
	d := json.NewDecoder(bytes.NewReader(raw))
	if !uniqueValue(d, 0) {
		return false
	}
	_, err := d.Token()
	return err == io.EOF
}
func uniqueValue(d *json.Decoder, depth int) bool {
	if depth > 6 {
		return false
	}
	t, err := d.Token()
	if err != nil {
		return false
	}
	delim, ok := t.(json.Delim)
	if !ok {
		return true
	}
	switch delim {
	case '{':
		keys := map[string]bool{}
		for d.More() {
			key, err := d.Token()
			s, ok := key.(string)
			if err != nil || !ok || keys[s] {
				return false
			}
			keys[s] = true
			if !uniqueValue(d, depth+1) {
				return false
			}
		}
		end, err := d.Token()
		return err == nil && end == json.Delim('}')
	case '[':
		for d.More() {
			if !uniqueValue(d, depth+1) {
				return false
			}
		}
		end, err := d.Token()
		return err == nil && end == json.Delim(']')
	default:
		return false
	}
}
