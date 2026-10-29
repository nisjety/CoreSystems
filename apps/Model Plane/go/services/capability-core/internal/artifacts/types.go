// Package artifacts documents the result-store expectations for large
// multimodal outputs — images, audio, video, and documents — that cannot
// be returned inline. Artifact records are produced by execution-core and
// inference-core runtimes and referenced by the public model-gateway
// responses via StorageURI. The catalog is seeded for the "triodelab"
// organization and is read-only.
package artifacts

// Kind enumerates supported artifact payload kinds.
type Kind string

// Supported artifact kinds.
const (
	KindImage    Kind = "image"
	KindAudio    Kind = "audio"
	KindVideo    Kind = "video"
	KindDocument Kind = "document"
)

// Artifact describes one result-store entry for a large multimodal output.
type Artifact struct {
	ID             string `json:"id"`
	IdempotencyKey string `json:"idempotencyKey"`
	OrgID          string `json:"orgId"`
	Kind           Kind   `json:"kind"`
	ModalityID     string `json:"modalityId"`
	ProducerRunID  string `json:"producerRunId,omitempty"`
	MimeType       string `json:"mimeType"`
	SizeBytes      int64  `json:"sizeBytes"`
	StorageURI     string `json:"storageUri"`
	RetentionDays  int    `json:"retentionDays"`
	CreatedAt      string `json:"createdAt"`
	Description    string `json:"description,omitempty"`
}

// Catalog is the read-only list of seeded artifact records.
type Catalog struct {
	Artifacts []Artifact `json:"artifacts"`
}

// Load returns the seeded artifact catalog.
func Load() Catalog { return catalog }
