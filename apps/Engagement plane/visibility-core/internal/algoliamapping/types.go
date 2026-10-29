// Package algoliamapping owns site-to-resource selection, never credentials.
package algoliamapping

import (
	"context"
	"errors"
	"regexp"

	"github.com/triodelab/engagement-plane/visibility-core/internal/sitesources"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

const (
	maxEntries = 32
	maxBytes   = 128 << 10
)

var (
	ErrInvalid         = errors.New("invalid Algolia site mapping")
	ErrForbidden       = errors.New("verified website ownership required")
	ErrUnavailable     = errors.New("Algolia access or resource check unavailable")
	idPattern          = regexp.MustCompile(`^web_[a-f0-9]{32}$`)
	orgPattern         = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._:-]{0,199}$`)
	connectionPattern  = regexp.MustCompile(`^conn_[A-Za-z0-9_-]{1,100}$`)
	appPattern         = regexp.MustCompile(`^[A-Za-z0-9]{1,64}$`)
	indexPattern       = regexp.MustCompile(`^[A-Za-z0-9_.-]{1,128}$`)
	generationPattern  = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._:-]{0,199}$`)
	fingerprintPattern = regexp.MustCompile(`^[a-f0-9]{64}$`)
)

type Registry interface {
	Resolve(string, string) (websites.Record, error)
	OrganizationID() string
}
type Client interface {
	Access(context.Context, string) (Access, error)
	Allowed(context.Context, string, string, string) (bool, error)
	Report(context.Context, websites.Record, Mapping) (sitesources.Report, error)
}
type Access struct {
	ConnectionID string `json:"connectionId"`
	Status       string `json:"status"`
	Generation   string `json:"generation"`
}
type Mapping struct {
	WebsiteID       string `json:"websiteId"`
	Site            string `json:"site"`
	WebsiteRevision int64  `json:"websiteRevision"`
	Status          string `json:"status"`
	ConnectionID    string `json:"connectionId,omitempty"`
	ApplicationID   string `json:"applicationId,omitempty"`
	IndexName       string `json:"indexName,omitempty"`
}
type entry struct {
	Mapping             Mapping `json:"mapping"`
	Version             string  `json:"version"`
	SnapshotFingerprint string  `json:"snapshotFingerprint,omitempty"`
	SnapshotGeneration  string  `json:"snapshotGeneration,omitempty"`
	SnapshotObservedAt  string  `json:"snapshotObservedAt,omitempty"`
	SnapshotDigest      string  `json:"snapshotDigest,omitempty"`
}
type diskState struct {
	Version        int     `json:"version"`
	OrganizationID string  `json:"organizationId"`
	Entries        []entry `json:"entries"`
}
