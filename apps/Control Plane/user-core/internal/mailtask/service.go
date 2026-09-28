package mailtask

import (
	"context"
	"crypto/rand"
	"encoding/base32"
	"errors"
	"fmt"
	"strings"
	"time"
)

// ErrNotFound is returned by ResolveAlias when no address row exists for the
// given alias at all.
var ErrNotFound = errors.New("mail task address not found")

// ErrRevoked is returned by ResolveAlias when the alias existed but its live
// window has ended. Kept distinct from ErrNotFound at THIS layer so an
// authenticated caller (e.g. a future admin/support surface) can tell the two
// apart -- the unauthenticated inbound-webhook path must never let that
// distinction leak, which is enforced by the HTTP handler collapsing both to
// one identical response, not by this package hiding the difference (spec §6,
// alias-enumeration risk).
var ErrRevoked = errors.New("mail task address revoked")

// ErrAliasCollision signals the vanishingly rare case where a freshly minted
// alias already exists. The alias space is 12 random bytes (2^96), so this
// exists purely as a defensive retry seam, never expected to fire in
// practice.
var ErrAliasCollision = errors.New("mail task alias collision")

// Repository is the storage seam Service depends on. The Postgres-backed
// implementation is in repository.go; tests substitute an in-memory fake so
// alias idempotency and revoke/regenerate semantics are verifiable without a
// live database.
type Repository interface {
	// ActiveForUser returns the live (non-revoked) alias for (orgID, userID),
	// or (nil, nil) when none exists yet.
	ActiveForUser(ctx context.Context, orgID, userID string) (*Address, error)
	// Insert stores a newly minted alias row. Returns ErrAliasCollision if the
	// alias token already exists, so the caller can retry with a fresh one.
	Insert(ctx context.Context, address Address) error
	// RevokeActive marks the live alias for (orgID, userID) revoked, if any
	// exists. A no-op (nil error) when there is nothing live to revoke.
	RevokeActive(ctx context.Context, orgID, userID string, revokedAt time.Time) error
	// ByAlias looks up one row by its opaque token, revoked or not --
	// ResolveAlias itself decides whether a revoked row is ErrRevoked or
	// ErrNotFound; the repository just reports what is stored.
	ByAlias(ctx context.Context, alias string) (*Address, error)
}

// aliasByteLength is the random-entropy size backing each minted alias.
// Spec §6: "aliases must be high-entropy (≥12 random bytes)".
const aliasByteLength = 12

// maxMintAttempts bounds the collision-retry loop in GenerateAlias. The alias
// space is large enough that this is a defensive ceiling, not an expected
// path.
const maxMintAttempts = 5

func newAliasToken() (string, error) {
	buf := make([]byte, aliasByteLength)
	if _, err := rand.Read(buf); err != nil {
		return "", fmt.Errorf("generate mail task alias entropy: %w", err)
	}
	// Base32 (RFC 4648, unpadded), lowercased: safe as an email local-part and
	// case-insensitive to match how mail clients and providers commonly treat
	// (and sometimes re-case) the local-part on relay.
	encoded := base32.StdEncoding.WithPadding(base32.NoPadding).EncodeToString(buf)
	return strings.ToLower(encoded), nil
}

// Service implements the alias lifecycle: GenerateAlias, ResolveAlias,
// RevokeAlias (mail-task spec §3.1).
type Service struct {
	repo Repository
}

// NewService wires Service against a Repository (normally *PostgresRepository
// from repository.go).
func NewService(repo Repository) *Service {
	return &Service{repo: repo}
}

// GenerateAlias is idempotent per (orgID, userID): a caller that already has
// a live alias gets it back unchanged instead of minting a second one, so
// repeated Settings-page loads (or retried requests) never orphan an earlier
// alias silently.
func (s *Service) GenerateAlias(ctx context.Context, orgID, userID string) (Address, error) {
	orgID = strings.TrimSpace(orgID)
	userID = strings.TrimSpace(userID)
	if orgID == "" || userID == "" {
		return Address{}, errors.New("org ID and user ID are required")
	}
	if s.repo == nil {
		return Address{}, errors.New("mail task repository is unavailable")
	}
	existing, err := s.repo.ActiveForUser(ctx, orgID, userID)
	if err != nil {
		return Address{}, err
	}
	if existing != nil {
		return *existing, nil
	}
	for attempt := 0; attempt < maxMintAttempts; attempt++ {
		alias, err := newAliasToken()
		if err != nil {
			return Address{}, err
		}
		address := Address{
			Alias:     alias,
			OrgID:     orgID,
			UserID:    userID,
			CreatedAt: time.Now().UTC(),
		}
		if err := s.repo.Insert(ctx, address); err != nil {
			if errors.Is(err, ErrAliasCollision) {
				continue
			}
			return Address{}, err
		}
		return address, nil
	}
	return Address{}, fmt.Errorf("could not mint a unique mail task alias after %d attempts", maxMintAttempts)
}

// ResolveAlias returns the live (org_id, user_id) an alias maps to.
// ErrNotFound and ErrRevoked are returned distinctly (see their doc
// comments); it is the HTTP layer's job to collapse them for an
// unauthenticated caller.
func (s *Service) ResolveAlias(ctx context.Context, alias string) (Address, error) {
	alias = strings.ToLower(strings.TrimSpace(alias))
	if alias == "" {
		return Address{}, ErrNotFound
	}
	if s.repo == nil {
		return Address{}, errors.New("mail task repository is unavailable")
	}
	address, err := s.repo.ByAlias(ctx, alias)
	if err != nil {
		return Address{}, err
	}
	if address == nil {
		return Address{}, ErrNotFound
	}
	if !address.IsLive() {
		return Address{}, ErrRevoked
	}
	return *address, nil
}

// RevokeAlias retires the current live alias and immediately mints its
// replacement, so the Settings "Generer på nytt / Regenerate" action (a
// DELETE followed by a GET, see MailTaskAddressSection.tsx) never observes a
// window with no address at all: the old alias stops resolving right away
// (ResolveAlias on it now returns ErrRevoked) and a fresh one is already live
// by the time the follow-up GET runs.
func (s *Service) RevokeAlias(ctx context.Context, orgID, userID string) error {
	orgID = strings.TrimSpace(orgID)
	userID = strings.TrimSpace(userID)
	if orgID == "" || userID == "" {
		return errors.New("org ID and user ID are required")
	}
	if s.repo == nil {
		return errors.New("mail task repository is unavailable")
	}
	if err := s.repo.RevokeActive(ctx, orgID, userID, time.Now().UTC()); err != nil {
		return err
	}
	_, err := s.GenerateAlias(ctx, orgID, userID)
	return err
}
