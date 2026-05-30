package objectstore

import (
	"context"
	"fmt"
	"strings"

	"github.com/triodelab/quarry/internal/config"
)

// Artifact describes one persisted binary artifact. The API returns this
// instead of embedding large blobs inline when object storage is configured.
type Artifact struct {
	Kind        string `json:"kind"`
	Provider    string `json:"provider"`
	Bucket      string `json:"bucket"`
	Key         string `json:"key"`
	URL         string `json:"url,omitempty"`
	ContentType string `json:"contentType"`
	Size        int64  `json:"size"`
	ETag        string `json:"etag,omitempty"`
}

type PutOptions struct {
	Kind        string
	ContentType string
	Metadata    map[string]string
}

type Store interface {
	Enabled() bool
	PutBytes(ctx context.Context, key string, data []byte, opts PutOptions) (*Artifact, error)
	Close() error
}

func NewFromConfig(cfg *config.Config) (Store, error) {
	if cfg == nil {
		return nil, nil
	}

	switch strings.ToLower(strings.TrimSpace(cfg.ArtifactStoreBackend)) {
	case "", "none":
		return nil, nil
	case "minio":
		return NewMinIOStore(MinIOConfig{
			Endpoint:      cfg.MinIOEndpoint,
			AccessKey:     cfg.MinIOAccessKey,
			SecretKey:     cfg.MinIOSecretKey,
			Bucket:        cfg.MinIOBucket,
			UseSSL:        cfg.MinIOUseSSL,
			Region:        cfg.MinIORegion,
			PublicBaseURL: cfg.MinIOPublicBaseURL,
		})
	default:
		return nil, fmt.Errorf("unsupported artifact store backend: %s", cfg.ArtifactStoreBackend)
	}
}
