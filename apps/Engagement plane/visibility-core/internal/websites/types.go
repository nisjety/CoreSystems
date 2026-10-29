// Package websites owns Marketing's durable, organization-bound website registry.
// Provider credentials remain exclusively in Integration Core.
package websites

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"regexp"
	"time"
)

var (
	ErrNotFound     = errors.New("website not found")
	ErrConflict     = errors.New("website changed; reload before saving")
	ErrInvalid      = errors.New("invalid website request")
	ErrUnavailable  = errors.New("website integration unavailable")
	idPattern       = regexp.MustCompile(`^web_[a-f0-9]{32}$`)
	identityPattern = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._:-]{0,199}$`)
	numericPattern  = regexp.MustCompile(`^[1-9][0-9]{0,19}$`)
	clarityPattern  = regexp.MustCompile(`^[a-z0-9]{4,32}$`)
)

type Mapping struct {
	Status        string `json:"status"`
	Property      string `json:"property,omitempty"`
	AccessLevel   string `json:"accessLevel,omitempty"`
	PropertyID    string `json:"propertyId,omitempty"`
	StreamID      string `json:"streamId,omitempty"`
	MeasurementID string `json:"measurementId,omitempty"`
	ReportingSite string `json:"reportingSite,omitempty"`
	ProjectID     string `json:"projectId,omitempty"`
	CheckedAt     string `json:"checkedAt,omitempty"`
}
type Integrations struct {
	SearchConsole Mapping `json:"searchConsole"`
	GA4           Mapping `json:"ga4"`
	Clarity       Mapping `json:"clarity"`
	PageSpeed     Mapping `json:"pageSpeed"`
}
type Ownership struct {
	Status    string `json:"status"`
	CheckedAt string `json:"checkedAt,omitempty"`
}
type Website struct {
	// Primary marks the organisation's primary site, the website org-core holds. It is set when the list is read
	// and never stored, so the registry has no copy of it that could go stale.
	Primary      bool         `json:"primary,omitempty"`
	ID           string       `json:"id"`
	Site         string       `json:"site"`
	Revision     int64        `json:"revision"`
	CreatedAt    string       `json:"createdAt"`
	UpdatedAt    string       `json:"updatedAt"`
	Ownership    Ownership    `json:"ownership"`
	Integrations Integrations `json:"integrations"`
}

// Record is a private persistence model, never a browser response.
type Record struct {
	GSCChoiceID            string        `json:"gscChoiceId,omitempty"`
	GA4ChoiceID            string        `json:"ga4ChoiceId,omitempty"`
	Website                Website       `json:"website"`
	GSCConnectionID        string        `json:"gscConnectionId,omitempty"`
	GA4ConnectionID        string        `json:"ga4ConnectionId,omitempty"`
	DNS                    *DNSChallenge `json:"dns,omitempty"`
	ConnectionsFingerprint string        `json:"connectionsFingerprint,omitempty"`
}
type Access struct {
	Status, ConnectionID, Property, AccessLevel, PropertyID, StreamID, MeasurementID, ReportingSite, CheckedAt string
}
type Evidence struct {
	SearchConsole, GA4     Access
	ConnectionsFingerprint string
}
type Discoverer func(context.Context, string) Evidence
type GoogleSession struct {
	SessionID        string `json:"sessionId"`
	AuthorizationURL string `json:"authorizationUrl"`
	ExpiresAt        string `json:"expiresAt"`
}
type GoogleActions struct {
	Begin  func(context.Context) (GoogleSession, error)
	Status func(context.Context, string) (string, error)
}
type DNSChallenge struct {
	Name      string `json:"name"`
	Value     string `json:"value"`
	ExpiresAt string `json:"expiresAt"`
}
type ChallengeResult struct {
	Website Website      `json:"website"`
	DNS     DNSChallenge `json:"dns"`
}

func newWebsite(site, stamp string) Website {
	var id [16]byte
	_, _ = rand.Read(id[:]) // crypto/rand.Read cannot return an error in supported Go.
	return Website{ID: "web_" + hex.EncodeToString(id[:]), Site: site, Revision: 1, CreatedAt: stamp, UpdatedAt: stamp,
		Ownership: Ownership{Status: "unverified"}, Integrations: Integrations{SearchConsole: Mapping{Status: "unmapped"}, GA4: Mapping{Status: "not_connected"}, Clarity: Mapping{Status: "not_connected"}, PageSpeed: Mapping{Status: "available"}}}
}
func timestamp(value string) bool { _, err := time.Parse(time.RFC3339Nano, value); return err == nil }
