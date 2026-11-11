package grant

import (
	"context"
	"strings"

	"github.com/jackc/pgx/v5"
)

// CreateHumanBrowse persists a non-ZDR human_browse grant (WS8.C). With
// MaxActivePerOwner set, the count (session and human_browse grants together)
// and the insert run under the per-owner advisory lock CreateSession uses, so
// the cap is exact under concurrent creates. A ZDR grant is refused: Routed
// sends those to the in-memory store.
func (s *PgStore) CreateHumanBrowse(ctx context.Context, p CreateHumanBrowseParams) (*Grant, error) {
	if p.Zdr {
		return nil, ErrZdrNotPersistable
	}
	if !validText(p.OrgID, p.OwnerID, p.SessionKey, p.ScopeURL) || !validText(p.DeniedDomains...) {
		return nil, ErrInvalidGrantText
	}
	denied, err := NormalizeDeniedDomains(p.DeniedDomains)
	if err != nil {
		return nil, err
	}
	id, err := newID(false)
	if err != nil {
		return nil, err
	}
	now := s.clock()
	g := &Grant{
		ID: id, Kind: KindHumanBrowse, OrgID: p.OrgID, OwnerID: p.OwnerID, SessionKey: p.SessionKey,
		ScopeURL: p.ScopeURL, PolicyMode: PolicyModeOpenWeb, DeniedDomains: denied,
		ExpiresAt: now.Add(p.TTL), AbsoluteExpiresAt: absoluteFrom(now, p.TTL, p.AbsoluteTTL),
	}
	err = s.inTx(ctx, "create_human_browse", func(tx pgx.Tx) error {
		if p.MaxActivePerOwner > 0 {
			if err := s.advisoryLock(ctx, tx, "create_human_browse", lockNsOwnerSessions, p.OrgID+":"+p.OwnerID); err != nil {
				return err
			}
			var active int
			if err := tx.QueryRow(ctx, `SELECT count(*) FROM browser_grants
				WHERE org_id = $1 AND owner_id = $2 AND kind IN ('session', 'human_browse')
				  AND revoked_at IS NULL AND expires_at >= $3`,
				p.OrgID, p.OwnerID, now).Scan(&active); err != nil {
				return s.dbErr(ctx, "create_human_browse", err)
			}
			if active >= p.MaxActivePerOwner {
				return ErrTooManyGrants
			}
		}
		args, err := insertArgs(g)
		if err != nil {
			return err
		}
		if _, err := tx.Exec(ctx, insertGrant, args...); err != nil {
			return s.dbErr(ctx, "create_human_browse", err)
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	return clone(g), nil
}

// Renew extends a session or human_browse grant by one TTL, never past its
// absolute expiry, with the rules of MemoryRepository.Renew. The row is locked
// FOR UPDATE, so a concurrent revoke either finishes first (the renewal then
// reads it revoked) or waits for the renewal and revokes the renewed row: a
// revoked grant is never extended.
func (s *PgStore) Renew(ctx context.Context, p RenewParams) (*Grant, error) {
	if p.OwnerID == "" {
		return nil, ErrRenewNotAllowed
	}
	if strings.HasPrefix(p.GrantID, zdrIDPrefix) || !validText(p.GrantID, p.OrgID, p.OwnerID) {
		return nil, ErrGrantNotFound
	}
	var out *Grant
	err := s.inTx(ctx, "renew", func(tx pgx.Tx) error {
		g, err := s.fetch(ctx, tx, "renew", p.GrantID, " FOR UPDATE")
		if err != nil {
			return err
		}
		now := s.clock()
		if err := scoped(g, p.OrgID, p.OwnerID, now); err != nil {
			return err
		}
		if !renewable(g) {
			return ErrRenewNotAllowed
		}
		next, err := renewedExpiry(g, now, p.TTL)
		if err != nil {
			return err
		}
		if _, err := tx.Exec(ctx, `UPDATE browser_grants SET expires_at = $2 WHERE id = $1`, g.ID, next); err != nil {
			return s.dbErr(ctx, "renew", err)
		}
		g.ExpiresAt = next
		out = g
		return nil
	})
	if err != nil {
		return nil, err
	}
	return clone(out), nil
}
