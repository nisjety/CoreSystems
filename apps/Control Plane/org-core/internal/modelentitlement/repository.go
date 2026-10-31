package modelentitlement

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/database"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

// ErrOrganizationNotFound is returned by a write for an organisation that does
// not exist. Reads of an unknown organisation return an empty policy set: there
// is nothing to refuse and the membership check decides who the caller is.
var ErrOrganizationNotFound = errors.New("organization not found")

// RevisionConflictError means the write named a revision that is no longer the
// current one. Current is the revision a caller should re-read and retry from.
type RevisionConflictError struct {
	Current int64
}

func (e *RevisionConflictError) Error() string {
	return fmt.Sprintf("model policy revision conflict: current revision is %d", e.Current)
}

// Repository stores policy sets. Every query runs inside WithOrgScope, so the
// row-level security of migration 021 is a second tenant filter behind the
// org_id predicates, the same way the rbac and org repositories work.
type Repository struct {
	db *database.DB
}

func NewRepository(db *database.DB) *Repository {
	return &Repository{db: db}
}

// postgresForeignKeyViolation is SQLSTATE 23503.
const postgresForeignKeyViolation = "23503"

// snapshot is the shape stored in the history table: the whole policy set as it
// stood, so a revision can be read back without replaying the ones before it.
type snapshot struct {
	OrgDefault *Policy           `json:"org_default"`
	Roles      map[string]Policy `json:"roles"`
}

// Get returns the organisation's policy set; Revision 0 and no layers when none
// was ever written.
func (r *Repository) Get(ctx context.Context, orgID string) (*PolicySet, error) {
	if r == nil || r.db == nil {
		return nil, errors.New("model entitlement repository not configured")
	}
	var set *PolicySet
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		loaded, err := loadSet(ctx, tx, orgID)
		set = loaded
		return err
	})
	if err != nil {
		return nil, err
	}
	return set, nil
}

// loadSet reads the revision and every policy row in ONE statement, so a write
// that commits in between cannot hand back revision N with the rows of N+1. The
// LEFT JOIN keeps an organisation whose last write cleared every layer.
func loadSet(ctx context.Context, tx pgx.Tx, orgID string) (*PolicySet, error) {
	rows, err := tx.Query(ctx, `
SELECT r.revision, r.updated_by, r.updated_at,
       p.scope_kind, p.scope_value, p.allow_mode, p.allow, p.deny,
       p.max_effort, p.min_privacy_tier, p.defaults
FROM org_model_policy_revisions r
LEFT JOIN org_model_policies p ON p.org_id = r.org_id
WHERE r.org_id = $1
ORDER BY p.scope_kind, p.scope_value`, orgID)
	if err != nil {
		return nil, fmt.Errorf("read model policies: %w", err)
	}
	defer rows.Close()

	set := &PolicySet{OrganizationID: orgID, Roles: map[string]Policy{}}
	for rows.Next() {
		var (
			updatedAt                        time.Time
			scopeKind, scopeValue, allowMode *string
			allow, deny                      []string
			maxEffort, minPrivacyTier        *string
			defaults                         []byte
		)
		if err := rows.Scan(&set.Revision, &set.UpdatedBy, &updatedAt,
			&scopeKind, &scopeValue, &allowMode, &allow, &deny,
			&maxEffort, &minPrivacyTier, &defaults); err != nil {
			return nil, fmt.Errorf("scan model policy: %w", err)
		}
		set.UpdatedAt = &updatedAt
		if scopeKind == nil {
			continue
		}
		policy := Policy{AllowMode: deref(allowMode), Allow: nonNil(allow), Deny: nonNil(deny), MaxEffort: deref(maxEffort), MinPrivacyTier: deref(minPrivacyTier)}
		if len(defaults) > 0 {
			var decoded map[string]SurfaceDefault
			if err := json.Unmarshal(defaults, &decoded); err != nil {
				return nil, fmt.Errorf("decode model policy defaults: %w", err)
			}
			if len(decoded) > 0 {
				policy.Defaults = decoded
			}
		}
		switch *scopeKind {
		case ScopeOrg:
			set.OrgDefault = &policy
		case ScopeRole:
			set.Roles[deref(scopeValue)] = policy
		}
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("read model policies: %w", err)
	}
	return set, nil
}

// Put replaces the organisation's whole policy set if expectedRevision is still
// the current revision, and records who did it and what it changed in the same
// transaction. req must already have passed ValidatePut.
//
// The revision row is the lock: every writer takes it FOR UPDATE first, so two
// concurrent writers with the same expected revision cannot both win, and the
// before-snapshot read afterwards cannot move under this transaction.
func (r *Repository) Put(ctx context.Context, orgID, actorID string, expectedRevision int64, req PutRequest) (*PolicySet, error) {
	if r == nil || r.db == nil {
		return nil, errors.New("model entitlement repository not configured")
	}
	var saved *PolicySet
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		current, err := lockRevision(ctx, tx, orgID)
		if err != nil {
			return err
		}
		if current != expectedRevision {
			return &RevisionConflictError{Current: current}
		}
		before, err := loadSet(ctx, tx, orgID)
		if err != nil {
			return err
		}

		next := current + 1
		if current == 0 {
			tag, err := tx.Exec(ctx, `
INSERT INTO org_model_policy_revisions (org_id, revision, updated_by)
VALUES ($1, $2, $3)
ON CONFLICT (org_id) DO NOTHING`, orgID, next, actorID)
			if err != nil {
				return mapWriteError(err)
			}
			if tag.RowsAffected() == 0 {
				// Another first write committed between the lock and this insert.
				latest, readErr := lockRevision(ctx, tx, orgID)
				if readErr != nil {
					return readErr
				}
				return &RevisionConflictError{Current: latest}
			}
		} else if _, err := tx.Exec(ctx, `
UPDATE org_model_policy_revisions
SET revision = $2, updated_by = $3, updated_at = NOW()
WHERE org_id = $1`, orgID, next, actorID); err != nil {
			return mapWriteError(err)
		}

		if _, err := tx.Exec(ctx, `DELETE FROM org_model_policies WHERE org_id = $1`, orgID); err != nil {
			return fmt.Errorf("clear model policies: %w", err)
		}
		if req.OrgDefault != nil {
			if err := insertPolicy(ctx, tx, orgID, ScopeOrg, "", *req.OrgDefault); err != nil {
				return err
			}
		}
		for role, policy := range req.Roles {
			if err := insertPolicy(ctx, tx, orgID, ScopeRole, role, policy); err != nil {
				return err
			}
		}

		after, err := loadSet(ctx, tx, orgID)
		if err != nil {
			return err
		}
		beforeJSON, err := json.Marshal(snapshot{OrgDefault: before.OrgDefault, Roles: before.Roles})
		if err != nil {
			return fmt.Errorf("encode model policy history: %w", err)
		}
		afterJSON, err := json.Marshal(snapshot{OrgDefault: after.OrgDefault, Roles: after.Roles})
		if err != nil {
			return fmt.Errorf("encode model policy history: %w", err)
		}
		if _, err := tx.Exec(ctx, `
INSERT INTO org_model_policy_history (org_id, revision, actor_id, before_set, after_set)
VALUES ($1, $2, $3, $4::jsonb, $5::jsonb)`, orgID, next, actorID, string(beforeJSON), string(afterJSON)); err != nil {
			return fmt.Errorf("record model policy history: %w", err)
		}
		saved = after
		return nil
	})
	if err != nil {
		return nil, err
	}
	return saved, nil
}

// lockRevision returns the organisation's current revision (0 when none) and
// holds its row until the transaction ends.
func lockRevision(ctx context.Context, tx pgx.Tx, orgID string) (int64, error) {
	var current int64
	err := tx.QueryRow(ctx, `
SELECT revision FROM org_model_policy_revisions WHERE org_id = $1 FOR UPDATE`, orgID).Scan(&current)
	if errors.Is(err, pgx.ErrNoRows) {
		return 0, nil
	}
	if err != nil {
		return 0, fmt.Errorf("lock model policy revision: %w", err)
	}
	return current, nil
}

func insertPolicy(ctx context.Context, tx pgx.Tx, orgID, scopeKind, scopeValue string, policy Policy) error {
	defaults := policy.Defaults
	if defaults == nil {
		defaults = map[string]SurfaceDefault{}
	}
	defaultsJSON, err := json.Marshal(defaults)
	if err != nil {
		return fmt.Errorf("encode model policy defaults: %w", err)
	}
	if _, err := tx.Exec(ctx, `
INSERT INTO org_model_policies
  (org_id, scope_kind, scope_value, allow_mode, allow, deny, max_effort, min_privacy_tier, defaults)
VALUES ($1, $2, $3, $4, $5, $6, NULLIF($7, ''), NULLIF($8, ''), $9::jsonb)`,
		orgID, scopeKind, scopeValue, policy.AllowMode, nonNil(policy.Allow), nonNil(policy.Deny),
		policy.MaxEffort, policy.MinPrivacyTier, string(defaultsJSON)); err != nil {
		return fmt.Errorf("write model policy: %w", err)
	}
	return nil
}

func mapWriteError(err error) error {
	var pgErr *pgconn.PgError
	if errors.As(err, &pgErr) && pgErr.Code == postgresForeignKeyViolation {
		return ErrOrganizationNotFound
	}
	return fmt.Errorf("write model policy revision: %w", err)
}

func deref(value *string) string {
	if value == nil {
		return ""
	}
	return *value
}

func nonNil(list []string) []string {
	if list == nil {
		return []string{}
	}
	return list
}
