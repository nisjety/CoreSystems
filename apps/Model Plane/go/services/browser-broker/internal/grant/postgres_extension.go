package grant

import (
	"context"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
)

// CreateOriginExtension persists a non-ZDR origin-extension grant (E3.2-e1)
// with the rules of MemoryRepository.CreateOriginExtension, in one
// transaction:
//
//  1. the ancestor chain is locked FOR SHARE top-down (root session first,
//     then the parent), exactly as CreateScopedSensitiveApproval does, so a
//     revoke of any ancestor either finishes first (the extension then fails)
//     or waits and then cascades to it;
//  2. a per-parent advisory lock serialises extensions under one parent, so
//     the child count and the host union (MaxExtensionHosts) are exact;
//  3. an identical live request returns the grant already issued.
//
// A ZDR request is refused (ErrZdrPostureChanged): a ZDR parent has a "z_" id
// and Routed sends its children to the in-memory store.
func (s *PgStore) CreateOriginExtension(ctx context.Context, p ExtensionParams) (*Grant, error) {
	if strings.HasPrefix(p.ParentGrantID, zdrIDPrefix) {
		return nil, ErrGrantNotFound
	}
	if p.Zdr {
		return nil, ErrZdrPostureChanged
	}
	if !validText(p.OrgID, p.OwnerID, p.SessionKey, p.ScopeURL, p.ParentGrantID, p.ConsentScope) ||
		!validText(p.AllowedDomains...) {
		return nil, ErrInvalidGrantText
	}
	var out *Grant
	err := s.inTx(ctx, "create_extension", func(tx pgx.Tx) error {
		first, err := s.fetch(ctx, tx, "create_extension", p.ParentGrantID, "")
		if err != nil {
			return err
		}
		if err := scoped(first, p.OrgID, p.OwnerID, s.clock()); err != nil {
			return err
		}
		if first.ParentGrantID != "" {
			root, err := s.fetch(ctx, tx, "create_extension", first.ParentGrantID, " FOR SHARE")
			if err != nil {
				return err
			}
			if err := scoped(root, p.OrgID, p.OwnerID, s.clock()); err != nil {
				return err
			}
		}
		parent, err := s.fetch(ctx, tx, "create_extension", p.ParentGrantID, " FOR SHARE")
		if err != nil {
			return err
		}
		now := s.clock()
		if err := scoped(parent, p.OrgID, p.OwnerID, now); err != nil {
			return err
		}
		fresh, err := planExtension(p, parent)
		if err != nil {
			return err
		}
		if err := s.advisoryLock(ctx, tx, "create_extension", lockNsParentExts, parent.ID); err != nil {
			return err
		}
		children, err := s.liveExtensions(ctx, tx, parent.ID, parent.OrgID, now)
		if err != nil {
			return err
		}
		reuse, err := decideExtension(children, fresh, p.ConsentScope)
		if err != nil {
			return err
		}
		if reuse != nil {
			out = reuse
			return nil
		}
		id, err := newID(false)
		if err != nil {
			return err
		}
		g := newExtensionGrant(id, p, parent, fresh, extensionExpiry(now, p.TTL, parent))
		args, err := insertArgs(g)
		if err != nil {
			return err
		}
		if _, err := tx.Exec(ctx, insertGrant, args...); err != nil {
			return s.dbErr(ctx, "create_extension", err)
		}
		out = g
		return nil
	})
	if err != nil {
		return nil, err
	}
	return clone(out), nil
}

// liveExtensions returns the live extension children of one parent.
func (s *PgStore) liveExtensions(ctx context.Context, tx pgx.Tx, parentID, orgID string, now time.Time) ([]*Grant, error) {
	rows, err := tx.Query(ctx, `SELECT id FROM browser_grants
		WHERE parent_grant_id = $1 AND org_id = $2 AND kind = 'origin_extension'
		  AND revoked_at IS NULL AND expires_at >= $3
		ORDER BY id`, parentID, orgID, now)
	if err != nil {
		return nil, s.dbErr(ctx, "create_extension", err)
	}
	var ids []string
	for rows.Next() {
		var id string
		if err := rows.Scan(&id); err != nil {
			rows.Close()
			return nil, s.dbErr(ctx, "create_extension", err)
		}
		ids = append(ids, id)
	}
	rows.Close()
	if err := rows.Err(); err != nil {
		return nil, s.dbErr(ctx, "create_extension", err)
	}
	children := make([]*Grant, 0, len(ids))
	for _, id := range ids {
		child, err := s.fetch(ctx, tx, "create_extension", id, "")
		if err != nil {
			return nil, err
		}
		children = append(children, child)
	}
	return children, nil
}
