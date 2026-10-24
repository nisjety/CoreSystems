package browsersites

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

// ErrOrganizationNotFound is returned by a write for an organisation that does not
// exist. Reads of an unknown organisation return the default policy: open mode and
// no rules, which is what an organisation that never wrote one has.
var ErrOrganizationNotFound = errors.New("organization not found")

// ErrPrefLimitReached means the member already holds MaxPrefsPerUser choices in
// this organisation and the write would add one more.
var ErrPrefLimitReached = errors.New("browser site choice limit reached")

// ErrPrefNotFound means the member holds no choice for the host.
var ErrPrefNotFound = errors.New("browser site choice not found")

// RevisionConflictError means the write named a revision that is no longer the
// current one. Current is the revision the caller should re-read and retry from.
type RevisionConflictError struct {
	Current int64
}

func (e *RevisionConflictError) Error() string {
	return fmt.Sprintf("browser site policy revision conflict: current revision is %d", e.Current)
}

// Repository stores the policy and the member choices. Every query runs inside
// WithOrgScope, so the row-level security of migration 023 is a second tenant
// filter behind the org_id predicates.
type Repository struct {
	db *database.DB
}

func NewRepository(db *database.DB) *Repository {
	return &Repository{db: db}
}

// postgresForeignKeyViolation is SQLSTATE 23503.
const postgresForeignKeyViolation = "23503"

// snapshot is the shape stored in the history table for a policy change.
type snapshot struct {
	Mode  string `json:"mode"`
	Rules []Rule `json:"rules"`
}

// Get returns the organisation's policy; Revision 0 and open mode with no rules
// when none was ever written.
func (r *Repository) Get(ctx context.Context, orgID string) (*PolicySet, error) {
	if r == nil || r.db == nil {
		return nil, errors.New("browser site repository not configured")
	}
	var set *PolicySet
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		loaded, err := loadPolicy(ctx, tx, orgID)
		set = loaded
		return err
	})
	if err != nil {
		return nil, err
	}
	return set, nil
}

// Snapshot reads the policy and, when userID is set, the caller's live choices for
// hosts, in one transaction so a write cannot land between the two reads.
func (r *Repository) Snapshot(ctx context.Context, orgID, userID string, hosts []string) (*PolicySet, map[string]Pref, error) {
	if r == nil || r.db == nil {
		return nil, nil, errors.New("browser site repository not configured")
	}
	var (
		set   *PolicySet
		prefs map[string]Pref
	)
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		loaded, err := loadPolicy(ctx, tx, orgID)
		if err != nil {
			return err
		}
		set = loaded
		if userID == "" {
			return nil
		}
		prefs, err = loadPrefsFor(ctx, tx, orgID, userID, hosts)
		return err
	})
	if err != nil {
		return nil, nil, err
	}
	return set, prefs, nil
}

// loadPolicy reads the mode, the revision and every rule in ONE statement, so a
// write that commits in between cannot hand back revision N with the rules of N+1.
// The LEFT JOIN keeps an organisation whose last write cleared every rule.
func loadPolicy(ctx context.Context, tx pgx.Tx, orgID string) (*PolicySet, error) {
	rows, err := tx.Query(ctx, `
SELECT p.revision, p.mode, p.updated_by, p.updated_at,
       r.host, r.rule, r.note
FROM org_browser_site_policy p
LEFT JOIN org_browser_site_rules r ON r.org_id = p.org_id
WHERE p.org_id = $1
ORDER BY r.host`, orgID)
	if err != nil {
		return nil, fmt.Errorf("read browser site policy: %w", err)
	}
	defer rows.Close()

	set := &PolicySet{OrganizationID: orgID, Mode: ModeOpen, Rules: []Rule{}}
	found := false
	for rows.Next() {
		var (
			updatedAt time.Time
			host      *string
			rule      *string
			note      *string
		)
		if err := rows.Scan(&set.Revision, &set.Mode, &set.UpdatedBy, &updatedAt, &host, &rule, &note); err != nil {
			return nil, fmt.Errorf("scan browser site policy: %w", err)
		}
		found = true
		set.UpdatedAt = &updatedAt
		if host != nil {
			set.Rules = append(set.Rules, Rule{Host: *host, Rule: deref(rule), Note: deref(note)})
		}
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("read browser site policy: %w", err)
	}
	if !found {
		return &PolicySet{OrganizationID: orgID, Mode: ModeOpen, Rules: []Rule{}}, nil
	}
	return set, nil
}

// Put replaces the organisation's policy if expectedRevision is still current and
// records who did it and what it changed in the same transaction. req must already
// have passed ValidatePut.
//
// The policy row is the lock: the writer takes it FOR UPDATE first, so two writers
// with one expected revision cannot both win. A member's choice takes it FOR SHARE,
// so a choice cannot be recorded against a policy that is being replaced.
func (r *Repository) Put(ctx context.Context, orgID, actorID string, expectedRevision int64, req PutRequest) (*PolicySet, error) {
	if r == nil || r.db == nil {
		return nil, errors.New("browser site repository not configured")
	}
	var saved *PolicySet
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		current, err := lockPolicy(ctx, tx, orgID, true)
		if err != nil {
			return err
		}
		if current != expectedRevision {
			return &RevisionConflictError{Current: current}
		}
		before, err := loadPolicy(ctx, tx, orgID)
		if err != nil {
			return err
		}

		next := current + 1
		if current == 0 {
			tag, err := tx.Exec(ctx, `
INSERT INTO org_browser_site_policy (org_id, mode, revision, updated_by)
VALUES ($1, $2, $3, $4)
ON CONFLICT (org_id) DO NOTHING`, orgID, req.Mode, next, actorID)
			if err != nil {
				return mapWriteError(err)
			}
			if tag.RowsAffected() == 0 {
				// Another first write committed between the lock and this insert.
				latest, readErr := lockPolicy(ctx, tx, orgID, true)
				if readErr != nil {
					return readErr
				}
				return &RevisionConflictError{Current: latest}
			}
		} else if _, err := tx.Exec(ctx, `
UPDATE org_browser_site_policy
SET mode = $2, revision = $3, updated_by = $4, updated_at = NOW()
WHERE org_id = $1`, orgID, req.Mode, next, actorID); err != nil {
			return mapWriteError(err)
		}

		if _, err := tx.Exec(ctx, `DELETE FROM org_browser_site_rules WHERE org_id = $1`, orgID); err != nil {
			return fmt.Errorf("clear browser site rules: %w", err)
		}
		for _, rule := range req.Rules {
			if _, err := tx.Exec(ctx, `
INSERT INTO org_browser_site_rules (org_id, host, rule, note)
VALUES ($1, $2, $3, $4)`, orgID, rule.Host, rule.Rule, rule.Note); err != nil {
				return mapWriteError(err)
			}
		}

		after, err := loadPolicy(ctx, tx, orgID)
		if err != nil {
			return err
		}
		beforeJSON, err := json.Marshal(snapshot{Mode: before.Mode, Rules: before.Rules})
		if err != nil {
			return fmt.Errorf("encode browser site history: %w", err)
		}
		afterJSON, err := json.Marshal(snapshot{Mode: after.Mode, Rules: after.Rules})
		if err != nil {
			return fmt.Errorf("encode browser site history: %w", err)
		}
		if _, err := tx.Exec(ctx, `
INSERT INTO org_browser_site_history (org_id, change_kind, revision, actor_id, before_set, after_set)
VALUES ($1, 'policy', $2, $3, $4::jsonb, $5::jsonb)`, orgID, next, actorID, string(beforeJSON), string(afterJSON)); err != nil {
			return fmt.Errorf("record browser site history: %w", err)
		}
		saved = after
		return nil
	})
	if err != nil {
		return nil, err
	}
	return saved, nil
}

// ListPrefs returns one member's choices in one organisation, with the policy
// revision they were read against.
func (r *Repository) ListPrefs(ctx context.Context, orgID, userID string) (PrefSet, error) {
	if r == nil || r.db == nil {
		return PrefSet{}, errors.New("browser site repository not configured")
	}
	out := PrefSet{OrganizationID: orgID, UserID: userID, Prefs: []Pref{}}
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		revision, err := readRevision(ctx, tx, orgID)
		if err != nil {
			return err
		}
		out.Revision = revision
		rows, err := tx.Query(ctx, `
SELECT host, level, expires_at, updated_at
FROM user_browser_site_prefs
WHERE org_id = $1 AND user_id = $2
ORDER BY host`, orgID, userID)
		if err != nil {
			return fmt.Errorf("read browser site choices: %w", err)
		}
		defer rows.Close()
		for rows.Next() {
			var pref Pref
			if err := rows.Scan(&pref.Host, &pref.Level, &pref.ExpiresAt, &pref.UpdatedAt); err != nil {
				return fmt.Errorf("scan browser site choice: %w", err)
			}
			out.Prefs = append(out.Prefs, pref)
		}
		return rows.Err()
	})
	if err != nil {
		return PrefSet{}, err
	}
	return out, nil
}

// PutPref records one member's choice for one host, if expectedRevision is still
// the current policy revision. The member's advisory lock serialises the count
// against the write, so the per-member limit holds under concurrency. Every write
// appends a history row whose actor is the member.
func (r *Repository) PutPref(ctx context.Context, orgID, userID string, expectedRevision int64, pref Pref) (Pref, error) {
	if r == nil || r.db == nil {
		return Pref{}, errors.New("browser site repository not configured")
	}
	var saved Pref
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		revision, err := lockPolicy(ctx, tx, orgID, false)
		if err != nil {
			return err
		}
		if revision != expectedRevision {
			return &RevisionConflictError{Current: revision}
		}
		if err := lockMember(ctx, tx, orgID, userID); err != nil {
			return err
		}

		before, existed, err := readPref(ctx, tx, orgID, userID, pref.Host)
		if err != nil {
			return err
		}
		if !existed {
			var count int
			if err := tx.QueryRow(ctx, `
SELECT count(*) FROM user_browser_site_prefs WHERE org_id = $1 AND user_id = $2`, orgID, userID).Scan(&count); err != nil {
				return fmt.Errorf("count browser site choices: %w", err)
			}
			if count >= MaxPrefsPerUser {
				return ErrPrefLimitReached
			}
		}

		if err := tx.QueryRow(ctx, `
INSERT INTO user_browser_site_prefs (org_id, user_id, host, level, expires_at, updated_at)
VALUES ($1, $2, $3, $4, $5, NOW())
ON CONFLICT (org_id, user_id, host)
DO UPDATE SET level = EXCLUDED.level, expires_at = EXCLUDED.expires_at, updated_at = NOW()
RETURNING host, level, expires_at, updated_at`,
			orgID, userID, pref.Host, pref.Level, pref.ExpiresAt).Scan(&saved.Host, &saved.Level, &saved.ExpiresAt, &saved.UpdatedAt); err != nil {
			return mapWriteError(err)
		}

		beforeJSON, err := encodePrefSnapshot(before, existed)
		if err != nil {
			return err
		}
		afterJSON, err := encodePrefSnapshot(saved, true)
		if err != nil {
			return err
		}
		return recordPrefHistory(ctx, tx, orgID, revision, userID, string(beforeJSON), string(afterJSON))
	})
	if err != nil {
		return Pref{}, err
	}
	return saved, nil
}

// DeletePref removes one member's choice for one host. ErrPrefNotFound when none
// exists; the deletion and its history row share one transaction.
func (r *Repository) DeletePref(ctx context.Context, orgID, userID, host string) error {
	if r == nil || r.db == nil {
		return errors.New("browser site repository not configured")
	}
	return r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		revision, err := lockPolicy(ctx, tx, orgID, false)
		if err != nil {
			return err
		}
		if err := lockMember(ctx, tx, orgID, userID); err != nil {
			return err
		}
		before, existed, err := readPref(ctx, tx, orgID, userID, host)
		if err != nil {
			return err
		}
		if !existed {
			return ErrPrefNotFound
		}
		if _, err := tx.Exec(ctx, `
DELETE FROM user_browser_site_prefs WHERE org_id = $1 AND user_id = $2 AND host = $3`, orgID, userID, host); err != nil {
			return fmt.Errorf("delete browser site choice: %w", err)
		}
		beforeJSON, err := encodePrefSnapshot(before, true)
		if err != nil {
			return err
		}
		return recordPrefHistory(ctx, tx, orgID, revision, userID, string(beforeJSON), "{}")
	})
}

// lockPolicy returns the organisation's current revision (0 when none) and holds
// the policy row until the transaction ends. forUpdate takes the exclusive lock an
// admin write needs; otherwise the shared lock a member write needs.
func lockPolicy(ctx context.Context, tx pgx.Tx, orgID string, forUpdate bool) (int64, error) {
	query := `SELECT revision FROM org_browser_site_policy WHERE org_id = $1 FOR SHARE`
	if forUpdate {
		query = `SELECT revision FROM org_browser_site_policy WHERE org_id = $1 FOR UPDATE`
	}
	var current int64
	err := tx.QueryRow(ctx, query, orgID).Scan(&current)
	if errors.Is(err, pgx.ErrNoRows) {
		return 0, nil
	}
	if err != nil {
		return 0, fmt.Errorf("lock browser site policy revision: %w", err)
	}
	return current, nil
}

// readRevision returns the organisation's current revision, 0 when none, without
// taking a lock: a read needs no more than the revision it reports.
func readRevision(ctx context.Context, tx pgx.Tx, orgID string) (int64, error) {
	var current int64
	err := tx.QueryRow(ctx, `SELECT revision FROM org_browser_site_policy WHERE org_id = $1`, orgID).Scan(&current)
	if errors.Is(err, pgx.ErrNoRows) {
		return 0, nil
	}
	if err != nil {
		return 0, fmt.Errorf("read browser site policy revision: %w", err)
	}
	return current, nil
}

// lockMember serialises one member's choice writes in one organisation, so the
// per-member count and the insert it guards cannot interleave.
func lockMember(ctx context.Context, tx pgx.Tx, orgID, userID string) error {
	if _, err := tx.Exec(ctx, `SELECT pg_advisory_xact_lock(hashtextextended($1, 0))`, orgID+"|"+userID); err != nil {
		return fmt.Errorf("lock browser site choices: %w", err)
	}
	return nil
}

func loadPrefsFor(ctx context.Context, tx pgx.Tx, orgID, userID string, hosts []string) (map[string]Pref, error) {
	rows, err := tx.Query(ctx, `
SELECT host, level, expires_at, updated_at
FROM user_browser_site_prefs
WHERE org_id = $1 AND user_id = $2 AND host = ANY($3)`, orgID, userID, hosts)
	if err != nil {
		return nil, fmt.Errorf("read browser site choices: %w", err)
	}
	defer rows.Close()
	prefs := make(map[string]Pref, len(hosts))
	for rows.Next() {
		var pref Pref
		if err := rows.Scan(&pref.Host, &pref.Level, &pref.ExpiresAt, &pref.UpdatedAt); err != nil {
			return nil, fmt.Errorf("scan browser site choice: %w", err)
		}
		prefs[pref.Host] = pref
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("read browser site choices: %w", err)
	}
	return prefs, nil
}

func readPref(ctx context.Context, tx pgx.Tx, orgID, userID, host string) (Pref, bool, error) {
	var pref Pref
	err := tx.QueryRow(ctx, `
SELECT host, level, expires_at, updated_at
FROM user_browser_site_prefs
WHERE org_id = $1 AND user_id = $2 AND host = $3`, orgID, userID, host).Scan(&pref.Host, &pref.Level, &pref.ExpiresAt, &pref.UpdatedAt)
	if errors.Is(err, pgx.ErrNoRows) {
		return Pref{}, false, nil
	}
	if err != nil {
		return Pref{}, false, fmt.Errorf("read browser site choice: %w", err)
	}
	return pref, true, nil
}

// encodePrefSnapshot is the content-free shape of one choice: host, level and
// expiry. An absent choice is the empty object.
func encodePrefSnapshot(pref Pref, present bool) ([]byte, error) {
	if !present {
		return []byte("{}"), nil
	}
	encoded, err := json.Marshal(struct {
		Host      string    `json:"host"`
		Level     string    `json:"level"`
		ExpiresAt time.Time `json:"expires_at"`
	}{pref.Host, pref.Level, pref.ExpiresAt})
	if err != nil {
		return nil, fmt.Errorf("encode browser site history: %w", err)
	}
	return encoded, nil
}

func recordPrefHistory(ctx context.Context, tx pgx.Tx, orgID string, revision int64, userID, beforeJSON, afterJSON string) error {
	if _, err := tx.Exec(ctx, `
INSERT INTO org_browser_site_history (org_id, change_kind, revision, actor_id, subject_user_id, before_set, after_set)
VALUES ($1, 'user_pref', $2, $3, $3, $4::jsonb, $5::jsonb)`, orgID, revision, userID, beforeJSON, afterJSON); err != nil {
		return fmt.Errorf("record browser site history: %w", err)
	}
	return nil
}

func mapWriteError(err error) error {
	var pgErr *pgconn.PgError
	if errors.As(err, &pgErr) && pgErr.Code == postgresForeignKeyViolation {
		return ErrOrganizationNotFound
	}
	return fmt.Errorf("write browser site policy: %w", err)
}

func deref(value *string) string {
	if value == nil {
		return ""
	}
	return *value
}
