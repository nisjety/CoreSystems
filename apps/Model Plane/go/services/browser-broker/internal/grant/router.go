package grant

import (
	"context"
	"errors"
	"strings"
	"time"
)

// Routed is the broker's Repository when a database is configured (WS1.C). It
// keeps Zero Data Retention grants out of the durable store (X-2):
//
//   - a session grant goes to ephemeral when it is ZDR, to persistent otherwise;
//   - every call that names an existing grant (GetScoped, RevokeScoped) or a
//     parent (Derive, CreateScopedSensitiveApproval) routes by the id's "z_"
//     prefix, so a ZDR id never reaches the database.
//
// A subtree never spans the two stores: a ZDR parent's children carry "z_"
// ids, a ZDR caller cannot derive from a persistent parent, and the persistent
// store refuses a ZDR-raised approval. So a revoke cascades within one store.
type Routed struct {
	persistent Repository
	ephemeral  Repository
}

var (
	_ Repository = (*Routed)(nil)
	_ Purger     = (*Routed)(nil)
)

// NewRouted returns a Routed over a durable store and an in-memory store.
func NewRouted(persistent, ephemeral Repository) *Routed {
	if persistent == nil || ephemeral == nil {
		panic("grant: NewRouted needs both a persistent and an ephemeral repository")
	}
	return &Routed{persistent: persistent, ephemeral: ephemeral}
}

// IsZdrID reports whether id names a Zero Data Retention grant.
func IsZdrID(id string) bool {
	return strings.HasPrefix(id, zdrIDPrefix)
}

func (r *Routed) byID(id string) Repository {
	if IsZdrID(id) {
		return r.ephemeral
	}
	return r.persistent
}

// CreateSession routes by the ZDR decision the server already made (request
// OR verified token, never lowered).
func (r *Routed) CreateSession(ctx context.Context, p CreateSessionParams) (*Grant, error) {
	if p.Zdr {
		return r.ephemeral.CreateSession(ctx, p)
	}
	return r.persistent.CreateSession(ctx, p)
}

// CreateScopedSensitiveApproval lives in its parent's store.
func (r *Routed) CreateScopedSensitiveApproval(ctx context.Context, p ApprovalParams) (*Grant, error) {
	return r.byID(p.ParentGrantID).CreateScopedSensitiveApproval(ctx, p)
}

// Derive lives in its parent's store.
func (r *Routed) Derive(ctx context.Context, p DeriveParams) (*Grant, error) {
	return r.byID(p.ParentGrantID).Derive(ctx, p)
}

// CreateHumanBrowse routes by the ZDR decision the server already made.
func (r *Routed) CreateHumanBrowse(ctx context.Context, p CreateHumanBrowseParams) (*Grant, error) {
	if p.Zdr {
		return r.ephemeral.CreateHumanBrowse(ctx, p)
	}
	return r.persistent.CreateHumanBrowse(ctx, p)
}

// Renew lives in the store of the grant it extends.
func (r *Routed) Renew(ctx context.Context, p RenewParams) (*Grant, error) {
	return r.byID(p.GrantID).Renew(ctx, p)
}

// GetScoped routes by the id prefix.
func (r *Routed) GetScoped(ctx context.Context, id, orgID, ownerID string) (*Grant, error) {
	return r.byID(id).GetScoped(ctx, id, orgID, ownerID)
}

// RevokeScoped routes by the id prefix; the subtree is in the same store.
func (r *Routed) RevokeScoped(ctx context.Context, id, orgID, ownerID, reason string) error {
	return r.byID(id).RevokeScoped(ctx, id, orgID, ownerID, reason)
}

// PurgeExpired purges both stores that support it and returns the total.
func (r *Routed) PurgeExpired(ctx context.Context, before time.Time) (int64, error) {
	var total int64
	var errs []error
	for _, repo := range []Repository{r.ephemeral, r.persistent} {
		if p, ok := repo.(Purger); ok {
			n, err := p.PurgeExpired(ctx, before)
			total += n
			if err != nil {
				errs = append(errs, err)
			}
		}
	}
	return total, errors.Join(errs...)
}
