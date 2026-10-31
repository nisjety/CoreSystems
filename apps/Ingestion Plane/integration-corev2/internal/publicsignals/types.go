// Package publicsignals owns bounded, read-only public research connectors.
// It does not crawl sites or fetch archive payloads; credentials stay local.
package publicsignals

import "errors"

var ErrInvalid = errors.New("invalid public signals request")
var ErrBusy = errors.New("public signals capacity exhausted")

type Period struct {
	StartDate string `json:"startDate"`
	EndDate   string `json:"endDate"`
}
type Metrics struct {
	LCPMs *float64 `json:"lcpMs"`
	INPMs *float64 `json:"inpMs"`
	CLS   *float64 `json:"cls"`
}
type Device struct {
	FormFactor      string   `json:"formFactor"`
	ReportingOrigin string   `json:"reportingOrigin,omitempty"`
	Status          string   `json:"status"`
	Reason          string   `json:"reason,omitempty"`
	Period          *Period  `json:"period,omitempty"`
	Metrics         *Metrics `json:"metrics,omitempty"`
}
type Capture struct {
	URL        string `json:"url"`
	CapturedAt string `json:"capturedAt"`
	HTTPStatus int    `json:"httpStatus"`
}
type CommonCrawl struct {
	ReportingHost string    `json:"reportingHost,omitempty"`
	Status        string    `json:"status"`
	Reason        string    `json:"reason,omitempty"`
	Index         string    `json:"index,omitempty"`
	Captures      []Capture `json:"captures"`
	Limit         int       `json:"limit"`
	Truncated     bool      `json:"truncated"`
}
type Report struct {
	Site        string      `json:"site"`
	Source      string      `json:"source"`
	ObservedAt  string      `json:"observedAt"`
	CrUX        []Device    `json:"crux"`
	CommonCrawl CommonCrawl `json:"commonCrawl"`
}
