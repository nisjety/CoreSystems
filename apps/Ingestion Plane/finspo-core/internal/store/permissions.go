package store

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"sort"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/finspo/internal/sharepoint"
)

// Permission mirrors one row of the permissions table.
type Permission struct {
	ID            uuid.UUID `json:"id"`
	ItemPK        uuid.UUID `json:"item_pk"`
	PrincipalID   string    `json:"principal_id,omitempty"`
	PrincipalType string    `json:"principal_type,omitempty"`
	PrincipalName string    `json:"principal_name,omitempty"`
	Roles         []string  `json:"roles"`
	LinkScope     string    `json:"link_scope,omitempty"`
	LinkType      string    `json:"link_type,omitempty"`
	InheritedFrom string    `json:"inherited_from,omitempty"`
	PermHash      string    `json:"perm_hash"`
	CapturedAt    time.Time `json:"captured_at"`
}

type Permissions struct {
	pool *pgxpool.Pool
}

// ReplaceAll deletes every permission row for itemPK and inserts the supplied
// entries inside a single transaction. Callers compute one PermissionEntry
// per principal/link before invoking. The function is idempotent: passing an
// empty slice clears all permissions for the item.
func (p *Permissions) ReplaceAll(ctx context.Context, itemPK uuid.UUID, entries []sharepoint.PermissionEntry) ([]Permission, error) {
	tx, err := p.pool.BeginTx(ctx, pgx.TxOptions{})
	if err != nil {
		return nil, fmt.Errorf("begin: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	if _, err := tx.Exec(ctx, `DELETE FROM permissions WHERE item_pk = $1`, itemPK); err != nil {
		return nil, fmt.Errorf("clear permissions for %s: %w", itemPK, err)
	}

	normalized := normalizePermissions(entries)

	out := make([]Permission, 0, len(normalized))
	for _, n := range normalized {
		hash := canonicalPermHash(n)
		raw, err := json.Marshal(n.raw)
		if err != nil {
			return nil, fmt.Errorf("marshal raw permission: %w", err)
		}

		var row Permission
		err = tx.QueryRow(ctx, `
INSERT INTO permissions (
    item_pk, principal_id, principal_type, principal_name,
    roles, link_scope, link_type, inherited_from, perm_hash, raw
) VALUES ($1, NULLIF($2,''), NULLIF($3,''), NULLIF($4,''),
          COALESCE($5::text[], '{}'::text[]),
          NULLIF($6,''), NULLIF($7,''), NULLIF($8,''),
          $9, COALESCE($10::jsonb, '{}'::jsonb))
ON CONFLICT (item_pk, perm_hash) DO UPDATE
   SET principal_name = EXCLUDED.principal_name,
       roles          = EXCLUDED.roles,
       raw            = EXCLUDED.raw,
       captured_at    = NOW()
RETURNING id, item_pk,
          COALESCE(principal_id,''), COALESCE(principal_type,''), COALESCE(principal_name,''),
          roles, COALESCE(link_scope,''), COALESCE(link_type,''),
          COALESCE(inherited_from,''), perm_hash, captured_at`,
			itemPK,
			n.principalID,
			n.principalType,
			n.principalName,
			n.roles,
			n.linkScope,
			n.linkType,
			n.inheritedFrom,
			hash,
			raw,
		).Scan(
			&row.ID, &row.ItemPK,
			&row.PrincipalID, &row.PrincipalType, &row.PrincipalName,
			&row.Roles, &row.LinkScope, &row.LinkType,
			&row.InheritedFrom, &row.PermHash, &row.CapturedAt,
		)
		if err != nil {
			return nil, fmt.Errorf("insert permission: %w", err)
		}
		out = append(out, row)
	}

	if err := tx.Commit(ctx); err != nil {
		return nil, fmt.Errorf("commit: %w", err)
	}
	return out, nil
}

// ListForItem returns every permission row currently attached to itemPK.
func (p *Permissions) ListForItem(ctx context.Context, itemPK uuid.UUID) ([]Permission, error) {
	rows, err := p.pool.Query(ctx, `
SELECT id, item_pk,
       COALESCE(principal_id,''), COALESCE(principal_type,''), COALESCE(principal_name,''),
       roles, COALESCE(link_scope,''), COALESCE(link_type,''),
       COALESCE(inherited_from,''), perm_hash, captured_at
  FROM permissions
 WHERE item_pk = $1
 ORDER BY captured_at`, itemPK)
	if err != nil {
		return nil, fmt.Errorf("list permissions: %w", err)
	}
	defer rows.Close()
	var out []Permission
	for rows.Next() {
		var row Permission
		if err := rows.Scan(
			&row.ID, &row.ItemPK,
			&row.PrincipalID, &row.PrincipalType, &row.PrincipalName,
			&row.Roles, &row.LinkScope, &row.LinkType,
			&row.InheritedFrom, &row.PermHash, &row.CapturedAt,
		); err != nil {
			return nil, fmt.Errorf("scan permission: %w", err)
		}
		out = append(out, row)
	}
	return out, rows.Err()
}

// ---------------------------------------------------------------------------
// normalization + hashing
// ---------------------------------------------------------------------------

type normalizedPermission struct {
	principalID   string
	principalType string
	principalName string
	roles         []string
	linkScope     string
	linkType      string
	inheritedFrom string
	raw           sharepoint.PermissionEntry
}

// normalizePermissions explodes one Graph permission entry into one or more
// normalized rows — one per distinct principal in grantedToIdentitiesV2, plus
// one row for link grants without an identity. Roles are sorted to make the
// permission hash stable regardless of Graph ordering.
func normalizePermissions(entries []sharepoint.PermissionEntry) []normalizedPermission {
	var out []normalizedPermission

	for _, e := range entries {
		roles := append([]string(nil), e.Roles...)
		sort.Strings(roles)

		inherited := ""
		if e.InheritedFrom != nil {
			inherited = e.InheritedFrom.ID
		}

		identities := collectIdentitySets(e)
		if len(identities) == 0 {
			// Pure link grant (no identity) — still record one row so the
			// link scope/type lives in Postgres.
			scope, typ := "", ""
			if e.Link != nil {
				scope = e.Link.Scope
				typ = e.Link.Type
			}
			out = append(out, normalizedPermission{
				roles:         roles,
				linkScope:     scope,
				linkType:      typ,
				inheritedFrom: inherited,
				raw:           e,
			})
			continue
		}

		for _, id := range identities {
			pid, kind, name := id.Principal()
			scope, typ := "", ""
			if e.Link != nil {
				scope = e.Link.Scope
				typ = e.Link.Type
			}
			out = append(out, normalizedPermission{
				principalID:   pid,
				principalType: kind,
				principalName: name,
				roles:         roles,
				linkScope:     scope,
				linkType:      typ,
				inheritedFrom: inherited,
				raw:           e,
			})
		}
	}

	return out
}

func collectIdentitySets(e sharepoint.PermissionEntry) []sharepoint.PermissionIdentitySet {
	var out []sharepoint.PermissionIdentitySet
	if e.GrantedToV2 != nil {
		out = append(out, *e.GrantedToV2)
	}
	out = append(out, e.GrantedToIdentitiesV2...)
	return out
}

// canonicalPermHash returns sha256(canonical_json(permission_summary)). The
// hash is stable across Graph ordering changes — re-pulling the same ACL
// produces the same hash, so the ON CONFLICT clause is exact.
func canonicalPermHash(n normalizedPermission) string {
	payload := []any{
		strings.ToLower(n.principalType),
		n.principalID,
		n.roles,
		strings.ToLower(n.linkScope),
		strings.ToLower(n.linkType),
		n.inheritedFrom,
	}
	b, err := json.Marshal(payload)
	if err != nil {
		// json.Marshal cannot fail for this payload shape.
		return ""
	}
	sum := sha256.Sum256(b)
	return hex.EncodeToString(sum[:])
}
