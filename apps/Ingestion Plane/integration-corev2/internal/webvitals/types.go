// Package webvitals reads bounded first-party, browser-reported performance
// exports. It never accepts caller-selected upstream URLs or browser secrets.
package webvitals

import (
	"bytes"
	"encoding/json"
	"errors"
	"regexp"
)

var (
	ErrInvalid     = errors.New("invalid web vitals request")
	ErrUnavailable = errors.New("web vitals source unavailable")
	ErrBusy        = errors.New("web vitals capacity busy")
	idPattern      = regexp.MustCompile(`^web_[a-f0-9]{32}$`)
	orgPattern     = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$`)
	samplePattern  = regexp.MustCompile(`^[a-f0-9]{8}-[a-f0-9]{4}-4[a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$`)
	pathPattern    = regexp.MustCompile(`^/(?:[a-z0-9-]+/)*[a-z0-9-]*$`)
)

type Input struct {
	WebsiteID       string `json:"websiteId"`
	Site            string `json:"site"`
	WebsiteRevision int64  `json:"websiteRevision"`
	Environment     string `json:"environment"`
}

func ParseInput(raw []byte) (Input, error) {
	var in Input
	if len(raw) > 2048 || UniqueJSON(raw) != nil || !exactKeys(raw, []string{"websiteId", "site", "websiteRevision", "environment"}, nil) {
		return in, ErrInvalid
	}
	d := json.NewDecoder(bytes.NewReader(raw))
	d.DisallowUnknownFields()
	if d.Decode(&in) != nil || !validInput(in) {
		return Input{}, ErrInvalid
	}
	return in, nil
}

type Sample struct {
	SampleID   string  `json:"sampleId"`
	Sequence   int     `json:"sequence"`
	Metric     string  `json:"metric"`
	Value      float64 `json:"value"`
	Viewport   string  `json:"viewport"`
	Path       string  `json:"path"`
	ReceivedAt string  `json:"receivedAt"`
}

func (s *Sample) UnmarshalJSON(raw []byte) error {
	type fields Sample
	var input struct {
		fields
		Value *float64 `json:"value"`
	}
	if json.Unmarshal(raw, &input) != nil || input.Value == nil {
		return ErrInvalid
	}
	*s = Sample(input.fields)
	s.Value = *input.Value
	return nil
}

type Report struct {
	WebsiteID       string   `json:"websiteId"`
	Site            string   `json:"site"`
	WebsiteRevision int64    `json:"websiteRevision"`
	Environment     string   `json:"environment"`
	ReportingOrigin string   `json:"reportingOrigin,omitempty"`
	Status          string   `json:"status"`
	Samples         []Sample `json:"samples"`
	ObservedAt      string   `json:"observedAt"`
}
