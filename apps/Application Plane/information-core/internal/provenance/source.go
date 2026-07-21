package provenance

// Source describes the authoritative upstream data used to produce a
// response. It is deliberately transport-neutral so every information-core
// adapter can expose the same freshness and coverage contract.
type Source struct {
	Provider          string  `json:"provider"`
	Dataset           string  `json:"dataset"`
	SourceURL         string  `json:"source_url"`
	License           string  `json:"license,omitempty"`
	RetrievedAt       string  `json:"retrieved_at"`
	EffectiveAt       string  `json:"effective_at,omitempty"`
	ExpiresAt         string  `json:"expires_at,omitempty"`
	Quality           string  `json:"quality"`
	Coverage          string  `json:"coverage"`
	Status            string  `json:"status"`
	UnavailableReason *string `json:"unavailable_reason"`
	APIVersion        string  `json:"api_version,omitempty"`
	Attribution       string  `json:"attribution,omitempty"`
	CRS               string  `json:"crs,omitempty"`
	Transformation    string  `json:"transformation_version,omitempty"`
}
