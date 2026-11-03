package orgkeys

import (
	"context"
	"errors"
	"fmt"
	"regexp"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/database"
)

const (
	// PurposeQuarryProfile is Quarry's browser-profile envelope (WS17.E1/E2b).
	// It is the only purpose today; a new purpose adds a case to contextAAD.
	PurposeQuarryProfile = "quarry.profile"

	// KeyIDPrefix marks a DEK wrapped by org-core; the version follows.
	KeyIDPrefix = "oc:"

	// WrappedDEKSize is nonce(12) || ciphertext(32) || tag(16).
	WrappedDEKSize = nonceSize + KeySize + gcmTagSize

	maxOrgIDLength    = 128
	maxKEKVersion     = 1_000_000
	readyRetryBackoff = 5 * time.Second
	auditFlushTimeout = 2 * time.Second
	auditFlushLimit   = 50
)

var profileIDPattern = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$`)

// AuditPublisher is satisfied by org-core's NATS client (PublishAudit), the same
// seam as org.AuditPublisher; declared here so this package does not import org.
type AuditPublisher interface {
	PublishAudit(ctx context.Context, subject, eventID string, payload map[string]any) error
}

// Service is the key API behind /internal/orgs/:orgId/keys.
type Service struct {
	store     Store
	master    MasterKeyProvider
	publisher AuditPublisher
	now       func() time.Time

	readyMu   sync.Mutex
	ready     bool
	lastCheck time.Time
	lastErr   error

	// unwraps counts successful unwraps: they are not audited one by one (the
	// hot path), only counted.
	unwraps atomic.Int64
}

// NewService builds the service over a store and a master key provider.
func NewService(store Store, master MasterKeyProvider) *Service {
	return &Service{store: store, master: master, now: time.Now}
}

// NewServiceFromEnv is the production constructor: env master keys (X-8 interim)
// over the Postgres store.
func NewServiceFromEnv(db *database.DB, getenv func(string) string) (*Service, error) {
	master, err := LoadEnvMasterKeys(getenv)
	if err != nil {
		return nil, err
	}
	return NewService(NewPostgresStore(db), master), nil
}

// SetAuditPublisher wires the audit dispatcher. Without one the intents stay in
// the outbox until FlushAudit runs with a publisher.
func (s *Service) SetAuditPublisher(publisher AuditPublisher) {
	s.publisher = publisher
}

// UnwrapCount reports how many unwraps succeeded since start.
func (s *Service) UnwrapCount() int64 {
	if s == nil {
		return 0
	}
	return s.unwraps.Load()
}

// Ready runs the master-key canary check once (and again after a failure, at
// most every few seconds). Every route calls it first.
func (s *Service) Ready(ctx context.Context) error {
	if s == nil || s.store == nil || s.master == nil {
		return fmt.Errorf("%w: key service is not configured", ErrUnavailable)
	}
	s.readyMu.Lock()
	defer s.readyMu.Unlock()
	if s.ready {
		return nil
	}
	if s.lastErr != nil && s.now().Sub(s.lastCheck) < readyRetryBackoff {
		return s.lastErr
	}
	s.lastCheck = s.now()
	if err := checkCanaries(ctx, s.master, s.store); err != nil {
		if !errors.Is(err, ErrUnavailable) {
			err = fmt.Errorf("%w: %v", ErrUnavailable, err)
		}
		s.lastErr = err
		return err
	}
	s.ready, s.lastErr = true, nil
	return nil
}

// DataKey is a freshly generated or rewrapped DEK. DEK is nil on a rewrap: the
// plaintext key never leaves org-core there.
type DataKey struct {
	KeyID      string
	KEKVersion int
	DEK        []byte
	WrappedDEK []byte
}

// Metadata is the GET /keys answer: versions and states, never key material.
type Metadata struct {
	OrgID         string
	State         string // "none", "active" or "destroyed"
	ActiveVersion int
	Versions      []VersionMetadata
}

// VersionMetadata describes one KEK version.
type VersionMetadata struct {
	Version     int
	State       string
	MasterKeyID string
	CreatedAt   time.Time
	RetiredAt   *time.Time
	DestroyedAt *time.Time
}

// FormatKeyID renders the key id a caller stores beside its envelope.
func FormatKeyID(version int) string {
	return KeyIDPrefix + strconv.Itoa(version)
}

// ParseKeyID reads "oc:<version>".
func ParseKeyID(keyID string) (int, error) {
	if !strings.HasPrefix(keyID, KeyIDPrefix) {
		return 0, ErrInvalidContext
	}
	digits := strings.TrimPrefix(keyID, KeyIDPrefix)
	if digits == "" || len(digits) > 7 || strings.HasPrefix(digits, "0") {
		return 0, ErrInvalidContext
	}
	version, err := strconv.Atoi(digits)
	if err != nil || version < 1 || version > maxKEKVersion {
		return 0, ErrInvalidContext
	}
	return version, nil
}

func validOrgID(orgID string) bool {
	return orgID != "" && orgID == strings.TrimSpace(orgID) && len(orgID) <= maxOrgIDLength &&
		!strings.ContainsAny(orgID, "|\x00")
}

// contextAAD binds a DEK to its org, purpose and object:
// "quarry.profile|org=<org>|profile=<id>". A DEK wrapped for one org or profile
// never opens for another. The context must carry exactly the purpose's keys.
func contextAAD(purpose, orgID string, keyContext map[string]string) ([]byte, error) {
	if !validOrgID(orgID) {
		return nil, ErrInvalidContext
	}
	switch purpose {
	case PurposeQuarryProfile:
		profileID, ok := keyContext["profile_id"]
		if !ok || len(keyContext) != 1 || !profileIDPattern.MatchString(profileID) {
			return nil, ErrInvalidContext
		}
		return []byte(purpose + "|org=" + orgID + "|profile=" + profileID), nil
	default:
		return nil, ErrInvalidContext
	}
}

func kekAAD(orgID string, version int, masterID string) []byte {
	return []byte("org-core.kek|org=" + orgID + "|v=" + strconv.Itoa(version) + "|master=" + masterID)
}

// buildKEK returns a BuildKEK that mints a random KEK sealed under the active
// master key.
func (s *Service) buildKEK(orgID string) BuildKEK {
	return func(version int) (string, []byte, error) {
		masterID := s.master.ActiveID()
		master, ok := s.master.Key(masterID)
		if !ok {
			return "", nil, fmt.Errorf("%w: active master key is not configured", ErrUnavailable)
		}
		defer zero(master)
		kek, err := newKey()
		if err != nil {
			return "", nil, err
		}
		defer zero(kek)
		wrapped, err := seal(master, kek, kekAAD(orgID, version, masterID))
		if err != nil {
			return "", nil, err
		}
		return masterID, wrapped, nil
	}
}

// openKEK unwraps a stored KEK. A KEK that does not open is a custody fault
// (wrong master key), so it is ErrUnavailable, never a caller error.
func (s *Service) openKEK(rec KEKRecord) ([]byte, error) {
	if rec.State == StateDestroyed || rec.Wrapped == nil {
		return nil, ErrDestroyed
	}
	master, ok := s.master.Key(rec.MasterKeyID)
	if !ok {
		return nil, fmt.Errorf("%w: master key %s is not configured", ErrUnavailable, rec.MasterKeyID)
	}
	defer zero(master)
	kek, err := open(master, rec.Wrapped, kekAAD(rec.OrgID, rec.Version, rec.MasterKeyID))
	if err != nil {
		return nil, fmt.Errorf("%w: kek %d does not open under master key %s", ErrUnavailable, rec.Version, rec.MasterKeyID)
	}
	return kek, nil
}

// GenerateDataKey mints a DEK, wraps it under the org's active KEK (created on
// first use under the org's lock) and returns both. The caller keeps only the
// wrapped DEK.
func (s *Service) GenerateDataKey(ctx context.Context, orgID, principal, purpose string, keyContext map[string]string) (DataKey, error) {
	aad, err := contextAAD(purpose, orgID, keyContext)
	if err != nil {
		return DataKey{}, err
	}
	if err := s.Ready(ctx); err != nil {
		return DataKey{}, err
	}
	rec, created, err := s.store.EnsureActiveKEK(ctx, orgID, s.buildKEK(orgID),
		AuditEvent{OrgID: orgID, Event: AuditKEKCreated, Principal: principal, Purpose: purpose})
	if err != nil {
		return DataKey{}, storeError(err)
	}
	if created {
		s.flushAudit()
	}
	kek, err := s.openKEK(rec)
	if err != nil {
		return DataKey{}, err
	}
	defer zero(kek)
	dek, err := newKey()
	if err != nil {
		return DataKey{}, err
	}
	wrapped, err := seal(kek, dek, aad)
	if err != nil {
		zero(dek)
		return DataKey{}, err
	}
	return DataKey{KeyID: FormatKeyID(rec.Version), KEKVersion: rec.Version, DEK: dek, WrappedDEK: wrapped}, nil
}

// unwrapDEK opens a wrapped DEK under the KEK version keyID names. The caller
// zeroes the returned DEK.
func (s *Service) unwrapDEK(ctx context.Context, orgID, principal, purpose string, keyContext map[string]string, keyID string, wrapped []byte) (dek []byte, activeVersion int, err error) {
	aad, err := contextAAD(purpose, orgID, keyContext)
	if err != nil {
		return nil, 0, err
	}
	version, err := ParseKeyID(keyID)
	if err != nil {
		return nil, 0, err
	}
	if len(wrapped) != WrappedDEKSize {
		return nil, 0, ErrInvalidContext
	}
	if err := s.Ready(ctx); err != nil {
		return nil, 0, err
	}
	rec, active, err := s.store.GetKEK(ctx, orgID, version)
	if err != nil {
		if errors.Is(err, ErrNoKey) {
			s.recordDenied(ctx, orgID, principal, purpose, version, "key_not_found")
		}
		return nil, 0, storeError(err)
	}
	kek, err := s.openKEK(rec)
	if err != nil {
		return nil, 0, err
	}
	defer zero(kek)
	dek, err = open(kek, wrapped, aad)
	if err != nil {
		// Wrong org, purpose or profile, or a tampered blob: one answer for all.
		s.recordDenied(ctx, orgID, principal, purpose, version, "context_mismatch")
		return nil, 0, ErrInvalidContext
	}
	return dek, active, nil
}

// Unwrap returns the plaintext DEK and the org's current KEK version, so a caller
// can tell a DEK wrapped under a retired KEK is due a rewrap.
func (s *Service) Unwrap(ctx context.Context, orgID, principal, purpose string, keyContext map[string]string, keyID string, wrapped []byte) ([]byte, int, error) {
	dek, active, err := s.unwrapDEK(ctx, orgID, principal, purpose, keyContext, keyID, wrapped)
	if err != nil {
		return nil, 0, err
	}
	s.unwraps.Add(1)
	return dek, active, nil
}

// Rewrap moves a wrapped DEK onto the org's active KEK. The DEK itself is
// unchanged and never leaves org-core.
func (s *Service) Rewrap(ctx context.Context, orgID, principal, purpose string, keyContext map[string]string, keyID string, wrapped []byte) (DataKey, error) {
	dek, _, err := s.unwrapDEK(ctx, orgID, principal, purpose, keyContext, keyID, wrapped)
	if err != nil {
		return DataKey{}, err
	}
	defer zero(dek)
	aad, err := contextAAD(purpose, orgID, keyContext)
	if err != nil {
		return DataKey{}, err
	}
	rec, created, err := s.store.EnsureActiveKEK(ctx, orgID, s.buildKEK(orgID),
		AuditEvent{OrgID: orgID, Event: AuditKEKCreated, Principal: principal, Purpose: purpose})
	if err != nil {
		return DataKey{}, storeError(err)
	}
	if created {
		s.flushAudit()
	}
	kek, err := s.openKEK(rec)
	if err != nil {
		return DataKey{}, err
	}
	defer zero(kek)
	rewrapped, err := seal(kek, dek, aad)
	if err != nil {
		return DataKey{}, err
	}
	return DataKey{KeyID: FormatKeyID(rec.Version), KEKVersion: rec.Version, WrappedDEK: rewrapped}, nil
}

// Rotate retires the active KEK and makes a new version active. DEKs wrapped
// under the old version still unwrap; Rewrap moves them over.
func (s *Service) Rotate(ctx context.Context, orgID, principal string) (VersionMetadata, error) {
	if !validOrgID(orgID) {
		return VersionMetadata{}, ErrInvalidContext
	}
	if err := s.Ready(ctx); err != nil {
		return VersionMetadata{}, err
	}
	rec, err := s.store.RotateKEK(ctx, orgID, s.buildKEK(orgID),
		AuditEvent{OrgID: orgID, Event: AuditKEKRotated, Principal: principal})
	if err != nil {
		return VersionMetadata{}, storeError(err)
	}
	s.flushAudit()
	return versionMetadata(rec), nil
}

// Metadata lists the org's KEK versions without key material.
func (s *Service) Metadata(ctx context.Context, orgID string) (Metadata, error) {
	if !validOrgID(orgID) {
		return Metadata{}, ErrInvalidContext
	}
	if err := s.Ready(ctx); err != nil {
		return Metadata{}, err
	}
	records, err := s.store.ListKEKs(ctx, orgID)
	if err != nil {
		return Metadata{}, storeError(err)
	}
	meta := Metadata{OrgID: orgID, State: "none", Versions: make([]VersionMetadata, 0, len(records))}
	for _, rec := range records {
		zero(rec.Wrapped)
		if rec.State == StateDestroyed {
			meta.State = StateDestroyed
		}
		if rec.State == StateActive && meta.State != StateDestroyed {
			meta.State = StateActive
			meta.ActiveVersion = rec.Version
		}
		if rec.Version > 0 {
			meta.Versions = append(meta.Versions, versionMetadata(rec))
		}
	}
	if meta.State == StateDestroyed {
		meta.ActiveVersion = 0
	}
	return meta, nil
}

// Destroy crypto-shreds every KEK of the org. It is irreversible: every DEK the
// org's KEKs wrapped is unrecoverable afterwards, and later calls answer
// ErrDestroyed. It does not need the canary: shredding must work even when the
// master key is gone.
func (s *Service) Destroy(ctx context.Context, orgID, principal string) (int, error) {
	if !validOrgID(orgID) {
		return 0, ErrInvalidContext
	}
	if s == nil || s.store == nil {
		return 0, fmt.Errorf("%w: key service is not configured", ErrUnavailable)
	}
	count, err := s.store.DestroyKEKs(ctx, orgID, AuditEvent{OrgID: orgID, Event: AuditKeysDestroy, Principal: principal})
	if err != nil {
		return 0, storeError(err)
	}
	s.flushAudit()
	return count, nil
}

// FlushAudit publishes pending audit intents (for a periodic loop in cmd/server).
func (s *Service) FlushAudit(ctx context.Context, limit int) (int, error) {
	if s == nil || s.store == nil {
		return 0, fmt.Errorf("%w: key service is not configured", ErrUnavailable)
	}
	if s.publisher == nil {
		return 0, errors.New("org key audit publisher is unavailable")
	}
	return s.store.FlushAudit(ctx, s.publisher.PublishAudit, limit)
}

// flushAudit is the best-effort publish after a committed change; a failure
// leaves the intent pending for the next flush.
func (s *Service) flushAudit() {
	if s.publisher == nil {
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), auditFlushTimeout)
	defer cancel()
	_, _ = s.FlushAudit(ctx, auditFlushLimit)
}

func (s *Service) recordDenied(ctx context.Context, orgID, principal, purpose string, version int, reason string) {
	if err := s.store.RecordAudit(ctx, AuditEvent{
		OrgID: orgID, Event: AuditUnwrapDenied, Principal: principal, Purpose: purpose,
		Reason: reason, KEKVersion: version,
	}); err == nil {
		s.flushAudit()
	}
}

func versionMetadata(rec KEKRecord) VersionMetadata {
	return VersionMetadata{
		Version: rec.Version, State: rec.State, MasterKeyID: rec.MasterKeyID,
		CreatedAt: rec.CreatedAt, RetiredAt: rec.RetiredAt, DestroyedAt: rec.DestroyedAt,
	}
}

// storeError keeps the typed errors and turns anything else (a database outage)
// into ErrUnavailable, without its text reaching the caller.
func storeError(err error) error {
	for _, typed := range []error{ErrOrgNotFound, ErrNoKey, ErrDestroyed, ErrInvalidContext, ErrUnavailable} {
		if errors.Is(err, typed) {
			return err
		}
	}
	return fmt.Errorf("%w: %v", ErrUnavailable, err)
}
