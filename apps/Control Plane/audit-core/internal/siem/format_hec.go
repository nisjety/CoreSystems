package siem

import (
	"encoding/json"
	"net/http"
	"time"
)

const (
	hecSource     = "verevon-audit"
	hecSourceType = "verevon:audit"
)

// hecEnvelope is the Splunk HTTP Event Collector event wrapper. The record
// inside is the same one the NDJSON destination receives.
type hecEnvelope struct {
	Time       float64 `json:"time"`
	Source     string  `json:"source"`
	SourceType string  `json:"sourcetype"`
	Event      any     `json:"event"`
}

func hecLine(item Item) ([]byte, error) {
	return json.Marshal(hecEnvelope{
		Time:       float64(item.At.UnixMilli()) / 1000,
		Source:     hecSource,
		SourceType: hecSourceType,
		Event:      record(item),
	})
}

// hecHeader authenticates with the HEC token the org stored as the
// destination secret. HEC takes concatenated event objects, one per line.
func hecHeader(secret string, _ []byte, _ time.Time) http.Header {
	header := http.Header{}
	header.Set("Content-Type", "application/json")
	header.Set("Authorization", "Splunk "+secret)
	return header
}
