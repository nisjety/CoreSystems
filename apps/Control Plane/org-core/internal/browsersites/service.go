package browsersites

import (
	"context"
	"errors"
	"strings"
	"time"
)

// Store is the persistence the service needs. *Repository satisfies it; the HTTP
// tests use an in-memory store so the real service and routes run without a
// database.
type Store interface {
	Get(ctx context.Context, orgID string) (*PolicySet, error)
	Put(ctx context.Context, orgID, actorID string, expectedRevision int64, req PutRequest) (*PolicySet, error)
	Snapshot(ctx context.Context, orgID, userID string, hosts []string) (*PolicySet, map[string]Pref, error)
	ListPrefs(ctx context.Context, orgID, userID string) (PrefSet, error)
	PutPref(ctx context.Context, orgID, userID string, expectedRevision int64, pref Pref) (Pref, error)
	DeletePref(ctx context.Context, orgID, userID, host string) error
	DeleteUserData(ctx context.Context, orgID, userID string) (int64, int64, error)
	DeleteRemovedMemberData(ctx context.Context, orgID, userID string) (int64, int64, error)
}

// ErrReservedUserID means the id is the history tombstone, which no member can hold, so
// an erasure for it is refused rather than matching the redacted rows.
var ErrReservedUserID = errors.New("user id is reserved")

// CodeInvalidSpaceID is the code a malformed space_id carries in its validation error.
const CodeInvalidSpaceID = "invalid_space_id"

// maxSpaceIDLength bounds one space_id, as the internal resolve route does.
const maxSpaceIDLength = 128

// Service is the one place the site policy is validated, written, resolved and
// explained. It takes its clock so a resolution's expiry is testable.
type Service struct {
	store Store
	now   func() time.Time
	// spaceFloor is nil until SetSpaceFloor wires a reader. With no reader every
	// Space-scoped resolve refuses every host, which is the fail-closed answer.
	spaceFloor SpaceFloorReader
	// auditOn is ORG_BROWSER_SITE_AUDIT_ENABLED, read once at startup (F-f06-11). With it off
	// a write queues no audit intent. auditPublisher is nil until SetAuditPublisher wires one.
	auditOn        bool
	auditPublisher AuditPublisher
}

func NewService(store Store, now func() time.Time) *Service {
	return &Service{store: store, now: now}
}

// SetSpaceFloor wires the reader for the Space floor. Pass a nil interface, never a
// typed nil pointer: a typed nil is not nil to the interface check in spaceEntitled.
func (s *Service) SetSpaceFloor(reader SpaceFloorReader) {
	s.spaceFloor = reader
}

// Get returns the stored policy for the admin read.
func (s *Service) Get(ctx context.Context, orgID string) (*PolicySet, error) {
	return s.store.Get(ctx, orgID)
}

// Put validates the body and replaces the organisation's policy. A body that fails
// validation is a *ValidationError and writes nothing. actorRole is the verified role of
// actorID; with the audit switch on the write queues its intent with that role, and a
// write without one is refused before it starts.
func (s *Service) Put(ctx context.Context, orgID, actorID, actorRole string, req PutRequest) (*PolicySet, error) {
	if strings.TrimSpace(actorID) == "" {
		return nil, errors.New("an actor id is required to write a browser site policy")
	}
	normalised, err := ValidatePut(req)
	if err != nil {
		return nil, err
	}
	ctx, err = s.writeContext(ctx, actorID, actorRole)
	if err != nil {
		return nil, err
	}
	saved, err := s.store.Put(ctx, orgID, actorID, *normalised.ExpectedRevision, normalised)
	if err != nil {
		return nil, err
	}
	s.flushAfterWrite(ctx, orgID)
	return saved, nil
}

// Prefs returns one member's choices in the organisation.
func (s *Service) Prefs(ctx context.Context, orgID, userID string) (PrefSet, error) {
	return s.store.ListPrefs(ctx, orgID, userID)
}

// SetPref records one member's choice for host. The host is canonicalised first, so
// the stored key is the one the resolver compares against. actorRole is the verified
// role of userID, as Put takes it.
func (s *Service) SetPref(ctx context.Context, orgID, userID, actorRole, host string, req PrefRequest) (Pref, error) {
	canonical, err := CanonicalHost(host)
	if err != nil {
		return Pref{}, err
	}
	input, err := ValidatePref(req, s.now())
	if err != nil {
		return Pref{}, err
	}
	ctx, err = s.writeContext(ctx, userID, actorRole)
	if err != nil {
		return Pref{}, err
	}
	saved, err := s.store.PutPref(ctx, orgID, userID, *req.ExpectedRevision, Pref{Host: canonical, Level: input.Level, ExpiresAt: input.ExpiresAt})
	if err != nil {
		return Pref{}, err
	}
	s.flushAfterWrite(ctx, orgID)
	return saved, nil
}

// DeletePref removes one member's choice for host. actorRole is the verified role of
// userID, as Put takes it.
func (s *Service) DeletePref(ctx context.Context, orgID, userID, actorRole, host string) error {
	canonical, err := CanonicalHost(host)
	if err != nil {
		return err
	}
	ctx, err = s.writeContext(ctx, userID, actorRole)
	if err != nil {
		return err
	}
	if err := s.store.DeletePref(ctx, orgID, userID, canonical); err != nil {
		return err
	}
	s.flushAfterWrite(ctx, orgID)
	return nil
}

// DeleteUserData erases one member's remembered choices in one organisation and redacts
// their history rows. It is the user self-erasure hook (F-f02-16) and erases whether or
// not the member is still active. It returns the counts only.
func (s *Service) DeleteUserData(ctx context.Context, orgID, userID string) (int64, int64, error) {
	orgID, userID, err := validateErasureSubject(orgID, userID)
	if err != nil {
		return 0, 0, err
	}
	return s.store.DeleteUserData(ctx, orgID, userID)
}

// DeleteRemovedMemberData erases a member's choices once their membership has ended. It
// erases nothing while the member is an active member (Repository.DeleteRemovedMemberData).
// It returns the counts only.
func (s *Service) DeleteRemovedMemberData(ctx context.Context, orgID, userID string) (int64, int64, error) {
	orgID, userID, err := validateErasureSubject(orgID, userID)
	if err != nil {
		return 0, 0, err
	}
	return s.store.DeleteRemovedMemberData(ctx, orgID, userID)
}

// validateErasureSubject trims both ids and refuses a blank one and the tombstone.
func validateErasureSubject(orgID, userID string) (string, string, error) {
	orgID = strings.TrimSpace(orgID)
	userID = strings.TrimSpace(userID)
	if orgID == "" || userID == "" {
		return "", "", errors.New("organization id and user id are required")
	}
	if userID == erasedTombstone {
		return "", "", ErrReservedUserID
	}
	return orgID, userID, nil
}

// Resolve answers for hosts for one caller. An empty userID is a service call and
// gets the admin's answer alone, and so does a userID that is not an active member of
// the organisation: a removed member's choice is not applied (F-f02-16). The lookup is
// not cached: a decision about one site is not where staleness is acceptable, and the
// broker fails closed on error.
func (s *Service) Resolve(ctx context.Context, orgID, userID string, hosts []string) (Resolution, error) {
	canonical, err := CanonicalHostList(hosts)
	if err != nil {
		return Resolution{}, err
	}
	set, prefs, err := s.store.Snapshot(ctx, orgID, strings.TrimSpace(userID), canonical)
	if err != nil {
		return Resolution{}, err
	}
	resolution := Resolve(*set, prefs, canonical, s.now())
	resolution.OrganizationID = orgID
	resolution.UserID = strings.TrimSpace(userID)
	return resolution, nil
}

// ResolveForSpace answers like Resolve, then applies the Space floor for the Space the
// call is made for (DEC-19 D9). Only the empty spaceID is an unscoped call, and it gets
// Resolve's answer unchanged. Any other value is a Space call: once trimmed it must be a
// well-formed space id, or the call is refused as a ValidationError before any read. The
// floor is then read, and an unentitled Space is refused every host, whatever the admin
// allows or the member chose. A floor that cannot be read, and a service with no reader,
// are unknown, and unknown refuses too.
func (s *Service) ResolveForSpace(ctx context.Context, orgID, userID, spaceID string, hosts []string) (Resolution, error) {
	if spaceID == "" {
		return s.Resolve(ctx, orgID, userID, hosts)
	}
	spaceID = strings.TrimSpace(spaceID)
	if !validSpaceID(spaceID) {
		return Resolution{}, &ValidationError{Errors: []FieldError{{
			Path:    "space_id",
			Code:    CodeInvalidSpaceID,
			Message: "space_id is 1 to 128 letters, digits, underscores, hyphens or colons",
		}}}
	}
	resolution, err := s.Resolve(ctx, orgID, userID, hosts)
	if err != nil {
		return Resolution{}, err
	}
	return ApplySpaceFloor(resolution, true, s.spaceEntitled(ctx, orgID, spaceID)), nil
}

// validSpaceID reports whether a trimmed space id is well formed: one to maxSpaceIDLength
// ASCII letters, digits, underscores, hyphens or colons, the characters Space ids are made of.
func validSpaceID(value string) bool {
	if value == "" || len(value) > maxSpaceIDLength {
		return false
	}
	for i := 0; i < len(value); i++ {
		c := value[i]
		if c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || c == '_' || c == '-' || c == ':' {
			continue
		}
		return false
	}
	return true
}

// spaceEntitled reads the floor for one Space. No reader, an error and a false answer
// are all not entitled.
func (s *Service) spaceEntitled(ctx context.Context, orgID, spaceID string) bool {
	if s.spaceFloor == nil {
		return false
	}
	entitled, err := s.spaceFloor.BrowserFloor(ctx, orgID, spaceID)
	return err == nil && entitled
}

// Explain answers for one host and reports the layers behind the answer: the
// admin's rule and the caller's own choice, with its expiry.
func (s *Service) Explain(ctx context.Context, orgID, userID, host string) (Explanation, error) {
	canonical, err := CanonicalHost(host)
	if err != nil {
		return Explanation{}, err
	}
	userID = strings.TrimSpace(userID)
	set, prefs, err := s.store.Snapshot(ctx, orgID, userID, []string{canonical})
	if err != nil {
		return Explanation{}, err
	}
	now := s.now()
	explanation := Explanation{
		OrganizationID: orgID,
		UserID:         userID,
		Revision:       set.Revision,
		Mode:           set.Mode,
		Decision:       Decide(*set, prefs, canonical, now),
		OrgRule:        ruleFor(*set, canonical),
	}
	if pref, ok := prefs[canonical]; ok {
		explanation.UserLevel = pref.Level
		expires := pref.ExpiresAt
		explanation.UserExpiresAt = &expires
	}
	return explanation, nil
}
