package store

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/triodelab/integration-corev2/internal/zdr"
)

// The ZDR capability record and the acknowledgement ledger of a model connection (Z-12).
//
// Two tables, deliberately apart from integration_connections:
//
//   - integration_connection_zdr is the CURRENT assessment of a connection (a side table, one
//     row per connection). It is written only through ApplyConnectionZDRAssessment, which
//     takes values computed by package zdr. No generic connection write (UpsertConnection,
//     ReconnectConnection, UpdateConnectionCredentials, the capability and provider-context
//     patches) touches it, so a read-modify-write that holds a stale Connection can never
//     write a stale digest back, and no client-controlled field can set a capability.
//   - integration_connection_zdr_acknowledgements is the LEDGER: what a user was shown and
//     accepted, bound to the digest of the assessment. A row is only ever inserted, and only
//     ever updated once, to record its revocation. It has NO foreign key to the connection:
//     connections are soft-deleted and the ledger must outlive any row it names.
//
// A connection with no record is unassessed. Every reader must treat that as cannot_verify,
// never as supported.

// ConnectionZDR is the current ZDR capability assessment of one model connection.
type ConnectionZDR struct {
	ConnectionID   string         `json:"connectionId"`
	ProviderKey    string         `json:"providerKey"`
	AuthMode       string         `json:"authMode"`
	Capability     zdr.Capability `json:"capability"`
	Basis          zdr.Basis      `json:"basis"`
	BasisRef       string         `json:"basisRef"`
	CatalogVersion int            `json:"catalogVersion"`
	// Digest is the opaque binding of every field above except ConnectionID and BasisRef,
	// computed by package zdr and by nothing else. Acknowledgements are bound to it.
	Digest string `json:"digest"`
	// CheckedAt is when this assessment was made. An unchanged re-assessment does not
	// rewrite it.
	CheckedAt time.Time `json:"checkedAt"`
	CreatedAt time.Time `json:"createdAt"`
	UpdatedAt time.Time `json:"updatedAt"`
}

// ConnectionZDRAssessment is what ApplyConnectionZDRAssessment is given: the outcome of
// zdr.Assessor.Assess for one connection.
type ConnectionZDRAssessment struct {
	ConnectionID   string
	ProviderKey    string
	AuthMode       string
	Capability     zdr.Capability
	Basis          zdr.Basis
	BasisRef       string
	CatalogVersion int
	Digest         string
	// CheckedAt defaults to now (UTC).
	CheckedAt time.Time
}

// NewConnectionZDRAssessment is the only way to build the input from an assessment.
func NewConnectionZDRAssessment(connectionID string, assessment zdr.Assessment) ConnectionZDRAssessment {
	return ConnectionZDRAssessment{
		ConnectionID:   connectionID,
		ProviderKey:    assessment.ProviderKey,
		AuthMode:       assessment.AuthMode,
		Capability:     assessment.Capability,
		Basis:          assessment.Basis,
		BasisRef:       assessment.BasisRef,
		CatalogVersion: assessment.CatalogVersion,
		Digest:         assessment.Digest,
	}
}

// ErrInvalidZDR marks input the store refuses before it touches any state.
var ErrInvalidZDR = errors.New("invalid zdr record")

func invalidZDR(format string, args ...any) error {
	return fmt.Errorf("%w: %s", ErrInvalidZDR, fmt.Sprintf(format, args...))
}

// Validate refuses an assessment that is incomplete or whose digest is not the digest of its
// own fields. Only package zdr computes digests, so a caller cannot invent one.
func (a ConnectionZDRAssessment) Validate() error {
	switch {
	case strings.TrimSpace(a.ConnectionID) == "":
		return invalidZDR("connection id is required")
	case strings.TrimSpace(a.ProviderKey) == "" || strings.TrimSpace(a.AuthMode) == "":
		return invalidZDR("provider key and auth mode are required")
	case !a.Capability.Valid():
		return invalidZDR("capability %q is not one of supported, not_supported, cannot_verify", a.Capability)
	case !a.Basis.Valid():
		return invalidZDR("basis %q is not a known basis", a.Basis)
	case strings.TrimSpace(a.BasisRef) == "":
		return invalidZDR("basis ref is required")
	case a.CatalogVersion < 1:
		return invalidZDR("catalog version must be positive")
	case !zdr.IsDigest(a.Digest):
		return invalidZDR("digest must be 64 lowercase hex characters")
	}
	want, err := zdr.Digest(a.ProviderKey, a.AuthMode, a.Capability, a.Basis, a.CatalogVersion)
	if err != nil {
		return invalidZDR("digest input: %v", err)
	}
	if want != a.Digest {
		return invalidZDR("digest does not match the provider key, auth mode, capability, basis and catalog version it is bound to")
	}
	return nil
}

func (a ConnectionZDRAssessment) record(now time.Time) ConnectionZDR {
	checkedAt := a.CheckedAt
	if checkedAt.IsZero() {
		checkedAt = now
	}
	checkedAt = checkedAt.UTC()
	return ConnectionZDR{
		ConnectionID:   a.ConnectionID,
		ProviderKey:    a.ProviderKey,
		AuthMode:       a.AuthMode,
		Capability:     a.Capability,
		Basis:          a.Basis,
		BasisRef:       a.BasisRef,
		CatalogVersion: a.CatalogVersion,
		Digest:         a.Digest,
		CheckedAt:      checkedAt,
		CreatedAt:      checkedAt,
		UpdatedAt:      checkedAt,
	}
}

// ConnectionZDRApplyResult reports what one ApplyConnectionZDRAssessment did.
type ConnectionZDRApplyResult struct {
	// Record is the assessment now in force.
	Record ConnectionZDR
	// Previous is the assessment before the call; nil when the connection had none.
	Previous *ConnectionZDR
	// Created is true when the connection had no record.
	Created bool
	// Changed is true when the digest differs from Previous: every acknowledgement given
	// against the old digest is no longer valid.
	Changed bool
	// Revoked lists the acknowledgements this call revoked (reason capability_changed): every
	// active one bound to a digest other than the current one.
	Revoked []ConnectionZDRAcknowledgement
}

// Acknowledgement revoke reasons. The set is closed; a new reason is a deliberate change here
// and in the Z-13 wording.
const (
	ZDRRevokeCapabilityChanged   = "capability_changed"
	ZDRRevokeConnectionRemoved   = "connection_removed"
	ZDRRevokeOwnerRevoked        = "owner_revoked"
	ZDRRevokeTextVersionOutdated = "text_version_superseded"
)

// Where a user was when they acknowledged.
const (
	ZDRAcknowledgedViaConnect  = "connect"
	ZDRAcknowledgedViaFirstUse = "first_use"
	ZDRAcknowledgedViaSettings = "settings"
)

var (
	zdrRevokeReasons = []string{ZDRRevokeCapabilityChanged, ZDRRevokeConnectionRemoved, ZDRRevokeOwnerRevoked, ZDRRevokeTextVersionOutdated}
	zdrAckVias       = []string{ZDRAcknowledgedViaConnect, ZDRAcknowledgedViaFirstUse, ZDRAcknowledgedViaSettings}
	zdrAckLocales    = []string{"no", "en"}
)

// ValidZDRRevokeReason reports whether reason is one of the closed revoke reasons.
func ValidZDRRevokeReason(reason string) bool { return containsString(zdrRevokeReasons, reason) }

// ConnectionZDRAcknowledgement is one row of the ledger: a user's acknowledgement that a
// connection cannot give ZDR, bound to the assessment digest they were shown.
type ConnectionZDRAcknowledgement struct {
	ID             string         `json:"id"`
	OrganizationID string         `json:"organizationId"`
	ConnectionID   string         `json:"connectionId"`
	UserID         string         `json:"userId"`
	ProviderKey    string         `json:"providerKey"`
	Capability     zdr.Capability `json:"capability"`
	Basis          zdr.Basis      `json:"basis"`
	CatalogVersion int            `json:"catalogVersion"`
	Digest         string         `json:"digest"`
	TextVersion    string         `json:"textVersion"`
	Locale         string         `json:"locale"`
	TextSHA256     string         `json:"textSha256"`
	// TextSnapshot is the full text the user was shown, kept so what they accepted can be
	// shown again exactly, whatever the wording files say later.
	TextSnapshot    string     `json:"textSnapshot"`
	OrgZDRAtAck     bool       `json:"orgZdrAtAck"`
	AcknowledgedVia string     `json:"acknowledgedVia"`
	RequestID       string     `json:"requestId,omitempty"`
	AcknowledgedAt  time.Time  `json:"acknowledgedAt"`
	RevokedAt       *time.Time `json:"revokedAt,omitempty"`
	RevokeReason    string     `json:"revokeReason,omitempty"`
}

// Active reports whether the acknowledgement has not been revoked. It says nothing about
// whether its digest is still current: that is compared at decision time.
func (a ConnectionZDRAcknowledgement) Active() bool { return a.RevokedAt == nil }

// NewConnectionZDRAcknowledgement is what InsertConnectionZDRAcknowledgement is given. The
// bound fields (provider, capability, basis, catalog version) are NOT inputs: the store
// copies them from the connection's current record in the same statement, so a row can never
// say something the assessment did not.
type NewConnectionZDRAcknowledgement struct {
	// ID defaults to a new identifier.
	ID             string
	OrganizationID string
	ConnectionID   string
	UserID         string
	// Digest is the digest the user was shown; the insert succeeds only while it is current.
	Digest          string
	TextVersion     string
	Locale          string
	TextSHA256      string
	TextSnapshot    string
	OrgZDRAtAck     bool
	AcknowledgedVia string
	RequestID       string
	// AcknowledgedAt defaults to now (UTC).
	AcknowledgedAt time.Time
}

// Validate refuses an acknowledgement that is incomplete or whose recorded hash is not the
// hash of its recorded text.
func (a NewConnectionZDRAcknowledgement) Validate() error {
	switch {
	case strings.TrimSpace(a.OrganizationID) == "" || strings.TrimSpace(a.ConnectionID) == "" || strings.TrimSpace(a.UserID) == "":
		return invalidZDR("organization, connection and user are required")
	case !zdr.IsDigest(a.Digest):
		return invalidZDR("digest must be 64 lowercase hex characters")
	case strings.TrimSpace(a.TextVersion) == "":
		return invalidZDR("text version is required")
	case !containsString(zdrAckLocales, a.Locale):
		return invalidZDR("locale %q is not one of no, en", a.Locale)
	case strings.TrimSpace(a.TextSnapshot) == "":
		return invalidZDR("the text the user was shown is required")
	case !containsString(zdrAckVias, a.AcknowledgedVia):
		return invalidZDR("acknowledged via %q is not one of connect, first_use, settings", a.AcknowledgedVia)
	}
	sum := sha256.Sum256([]byte(a.TextSnapshot))
	if a.TextSHA256 != hex.EncodeToString(sum[:]) {
		return invalidZDR("text sha256 is not the hash of the recorded text")
	}
	return nil
}

// ConnectionZDRStore is the surface the ZDR record and ledger are reached through. It is part
// of both Repository and AuditTransaction: a digest change and the audit row that records it
// must commit together, so callers run it inside WithAuditTransaction.
type ConnectionZDRStore interface {
	// ApplyConnectionZDRAssessment records the assessment of a connection. It is idempotent:
	// an assessment whose digest equals the stored one writes nothing but still revokes any
	// active acknowledgement bound to another digest, so a missed revocation heals. When the
	// digest changed, the record is updated and every active acknowledgement bound to the old
	// digest is revoked with reason capability_changed, in the same transaction as the caller's
	// audit row. ErrNotFound when the connection does not exist; ErrInvalidZDR for a malformed
	// assessment.
	ApplyConnectionZDRAssessment(ctx context.Context, assessment ConnectionZDRAssessment) (ConnectionZDRApplyResult, error)
	// GetConnectionZDR returns the current assessment; ErrNotFound when the connection has
	// none (unassessed: treat as cannot_verify).
	GetConnectionZDR(ctx context.Context, connectionID string) (ConnectionZDR, error)
	// InsertConnectionZDRAcknowledgement appends an acknowledgement for the connection's owner.
	// It succeeds only while the digest the user was shown is the current one, the connection
	// is live and (organization, user) own it, and the capability requires an acknowledgement.
	// created is false when an active acknowledgement for the same connection and digest
	// already exists, which is returned unchanged (an idempotent replay). ErrConflict means a
	// stale digest; ErrNotFound means no such connection, record or owner.
	InsertConnectionZDRAcknowledgement(ctx context.Context, ack NewConnectionZDRAcknowledgement) (saved ConnectionZDRAcknowledgement, created bool, err error)
	// ListConnectionZDRAcknowledgements returns a connection's ledger rows, newest first.
	ListConnectionZDRAcknowledgements(ctx context.Context, connectionID string, activeOnly bool) ([]ConnectionZDRAcknowledgement, error)
	// RevokeConnectionZDRAcknowledgements revokes every active acknowledgement of a connection
	// with a closed reason and returns the rows it revoked (none when there was nothing active).
	// A revoked row is final.
	RevokeConnectionZDRAcknowledgements(ctx context.Context, connectionID, reason string, at time.Time) ([]ConnectionZDRAcknowledgement, error)
}

func containsString(list []string, value string) bool {
	for _, item := range list {
		if item == value {
			return true
		}
	}
	return false
}

func cloneZDRAcknowledgement(ack ConnectionZDRAcknowledgement) ConnectionZDRAcknowledgement {
	if ack.RevokedAt != nil {
		revoked := *ack.RevokedAt
		ack.RevokedAt = &revoked
	}
	return ack
}

func cloneZDRAcknowledgements(input []ConnectionZDRAcknowledgement) []ConnectionZDRAcknowledgement {
	output := make([]ConnectionZDRAcknowledgement, len(input))
	for index, ack := range input {
		output[index] = cloneZDRAcknowledgement(ack)
	}
	return output
}
