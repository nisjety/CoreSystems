// Package modalities documents the broad multimodal AI surface exposed by
// the model plane — chat, completions, images, speech, translation,
// documents, video, and realtime — as a read-only catalog describing
// runtime routing and provider fallback expectations. Public ingress is
// owned by model-gateway; provider routing and streaming are owned by
// inference-core; multimodal orchestration is owned by execution-core.
// The catalog is seeded for the "triodelab" organization.
package modalities

// Kind enumerates the supported multimodal AI surfaces.
type Kind string

// Supported modality kinds.
const (
	KindChat        Kind = "chat"
	KindCompletions Kind = "completions"
	KindImages      Kind = "images"
	KindSpeech      Kind = "speech"
	KindTranslation Kind = "translation"
	KindDocuments   Kind = "documents"
	KindVideo       Kind = "video"
	KindRealtime    Kind = "realtime"
)

// Modality is a durable description of one multimodal AI surface, including
// the routing plan used by inference-core when providers fail over.
type Modality struct {
	ID             string   `json:"id"`
	IdempotencyKey string   `json:"idempotencyKey"`
	OrgID          string   `json:"orgId"`
	Kind           Kind     `json:"kind"`
	PublicOwner    string   `json:"publicOwner"`
	RuntimeOwner   string   `json:"runtimeOwner"`
	Inputs         []string `json:"inputs,omitempty"`
	Outputs        []string `json:"outputs,omitempty"`
	Streaming      bool     `json:"streaming"`
	PrimaryRoute   string   `json:"primaryRoute"`
	Fallbacks      []string `json:"fallbacks,omitempty"`
	ArtifactStore  string   `json:"artifactStore,omitempty"`
	Description    string   `json:"description,omitempty"`
}

// Catalog is the read-only list of multimodal surfaces.
type Catalog struct {
	Modalities []Modality `json:"modalities"`
}

// Load returns the seeded modality catalog.
func Load() Catalog { return catalog }
