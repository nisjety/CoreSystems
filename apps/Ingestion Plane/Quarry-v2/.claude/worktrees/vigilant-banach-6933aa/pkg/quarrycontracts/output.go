package quarrycontracts

import "time"

type NormalizedOutput struct {
	RunID       ID             `json:"run_id"`
	URL         URLTriple      `json:"url"`
	Status      uint16         `json:"status"`
	FetchedAt   time.Time      `json:"fetched_at"`
	Fingerprint string         `json:"fingerprint"`
	Formats     OutputFormats  `json:"formats"`
	Change      ChangeInfo     `json:"change"`
	Metadata    PageMetadata   `json:"metadata"`
	Driver      DriverInfo     `json:"driver"`
}

type URLTriple struct {
	Requested string  `json:"requested"`
	Final     string  `json:"final"`
	Canonical *string `json:"canonical,omitempty"`
}

type OutputFormats struct {
	HTML       *FormatRef  `json:"html,omitempty"`
	Markdown   *FormatRef  `json:"markdown,omitempty"`
	Raw        *FormatRef  `json:"raw,omitempty"`
	Links      []Link      `json:"links,omitempty"`
	Screenshot *FormatRef  `json:"screenshot,omitempty"`
	PDF        *FormatRef  `json:"pdf,omitempty"`
	Extract    *ExtractRef `json:"extract,omitempty"`
}

type FormatRef struct {
	ArtifactID ID     `json:"artifact_id"`
	Bytes      uint64 `json:"bytes"`
}

type ExtractRef struct {
	ArtifactID ID      `json:"artifact_id"`
	SchemaID   *string `json:"schema_id,omitempty"`
}

type Link struct {
	Href string  `json:"href"`
	Text *string `json:"text,omitempty"`
	Rel  *string `json:"rel,omitempty"`
}

type ChangeInfo struct {
	Status           ChangeStatus `json:"status"`
	PrevFingerprint  *string      `json:"prev_fingerprint,omitempty"`
}

type ChangeStatus string
const (
	ChangeNew       ChangeStatus = "new"
	ChangeChanged   ChangeStatus = "changed"
	ChangeUnchanged ChangeStatus = "unchanged"
)

type PageMetadata struct {
	Title       *string `json:"title,omitempty"`
	Lang        *string `json:"lang,omitempty"`
	ContentType *string `json:"content_type,omitempty"`
}

type DriverInfo struct {
	Kind       DriverKind `json:"kind"`
	DurationMs uint64     `json:"duration_ms"`
}

type DriverKind string
const (
	DriverStatic  DriverKind = "static"
	DriverBrowser DriverKind = "browser"
	DriverTLS     DriverKind = "tls"
)
