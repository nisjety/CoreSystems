package grant

import (
	"context"
	"errors"
	"log/slog"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"

	"github.com/triodelab/model-plane/services/browser-broker/internal/telemetry"
)

var (
	// ErrStoreUnavailable means the durable grant store failed. It carries no
	// SQL text, row value or domain; callers map it to a retryable error and
	// never treat it as an allow.
	ErrStoreUnavailable = errors.New("browser grant store unavailable")
	// ErrZdrNotPersistable means a Zero Data Retention grant reached the
	// durable store. Routed never sends one; the store refuses it anyway.
	ErrZdrNotPersistable = errors.New("zero data retention grant cannot be persisted")
)

// Advisory lock namespaces (the first key of pg_advisory_xact_lock(int, int)),
// so the per-owner and per-parent locks never share a key.
const (
	lockNsOwnerSessions int32 = 0x62620001
	lockNsParentRuns    int32 = 0x62620002
)

// Purger removes grants that expired before a cutoff.
type Purger interface {
	PurgeExpired(ctx context.Context, before time.Time) (int64, error)
}

// PgStore is the durable Repository (WS1.C, table browser_grants). It never
// holds a ZDR grant: there is no zdr column, a "z_" id is answered NotFound
// without a query, and a ZDR create is refused. Routed sends ZDR grants to the
// in-memory store instead.
//
// Every expiry is computed in Go from s.now and passed as a parameter, exactly
// as MemoryRepository does, so both stores share one contract (and one fake
// clock in tests). The broker runs as one replica (X-2), so there is no
// cross-host clock to disagree with.
type PgStore struct {
	pool *pgxpool.Pool
	now  func() time.Time
	// Test hooks, nil in production. afterParentLock runs inside Derive's
	// transaction once the parent is locked FOR SHARE; afterRootLock runs
	// inside RevokeScoped's transaction once the root is locked FOR UPDATE.
	afterParentLock func()
	afterRootLock   func()
}

var (
	_ Repository = (*PgStore)(nil)
	_ Purger     = (*PgStore)(nil)
)

// NewPgStore returns a PgStore over pool. The schema is applied by the
// container entrypoint (migrations/*.up.sql), not here.
func NewPgStore(pool *pgxpool.Pool) *PgStore {
	return &PgStore{pool: pool, now: time.Now}
}

// Ping checks the database for /readyz.
func (s *PgStore) Ping(ctx context.Context) error {
	if err := s.pool.Ping(ctx); err != nil {
		return s.dbErr(ctx, "ping", err)
	}
	return nil
}

// clock is the store's now at the database's precision, so a grant returned
// by a create equals the same grant read back.
func (s *PgStore) clock() time.Time {
	return s.now().UTC().Truncate(time.Microsecond)
}

// dbErr records a database failure and hides its text. A cancelled or timed
// out request is passed through uncounted: it is the caller leaving, not the
// store failing.
func (s *PgStore) dbErr(ctx context.Context, op string, err error) error {
	if errors.Is(err, context.Canceled) || errors.Is(err, context.DeadlineExceeded) {
		return err
	}
	telemetry.GrantStoreErrorsTotal.Add(context.WithoutCancel(ctx), 1,
		metric.WithAttributes(attribute.String("op", op)))
	sqlState := ""
	var pgErr *pgconn.PgError
	if errors.As(err, &pgErr) {
		sqlState = pgErr.Code
	}
	// The SQLSTATE class only: a PostgreSQL message can quote row values.
	slog.Warn("browser grant store error", "op", op, "sqlstate", sqlState)
	return ErrStoreUnavailable
}

type querier interface {
	QueryRow(ctx context.Context, sql string, args ...any) pgx.Row
	Exec(ctx context.Context, sql string, args ...any) (pgconn.CommandTag, error)
}

const grantColumns = `id, kind, org_id, owner_id, session_key, scope_url, allowed_domains,
	allowed_actions, allowed_frame_ids, allowed_dialog_ids, allowed_artifact_ids,
	COALESCE(parent_grant_id, ''), expires_at, revoked_at IS NOT NULL, COALESCE(revoked_reason, '')`

// fetch reads one grant; lock is "", " FOR SHARE" or " FOR UPDATE". A
// missing row is ErrGrantNotFound.
func (s *PgStore) fetch(ctx context.Context, q querier, op, id, lock string) (*Grant, error) {
	var g Grant
	var kind string
	err := q.QueryRow(ctx, `SELECT `+grantColumns+` FROM browser_grants WHERE id = $1`+lock, id).Scan(
		&g.ID, &kind, &g.OrgID, &g.OwnerID, &g.SessionKey, &g.ScopeURL, &g.AllowedDomains,
		&g.AllowedActions, &g.AllowedFrameIDs, &g.AllowedDialogIDs, &g.AllowedArtifactIDs,
		&g.ParentGrantID, &g.ExpiresAt, &g.Revoked, &g.RevokedReason,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrGrantNotFound
	}
	if err != nil {
		return nil, s.dbErr(ctx, op, err)
	}
	g.Kind = Kind(kind)
	return &g, nil
}

// scoped applies the same visibility rules as MemoryRepository.getScopedLocked.
func scoped(g *Grant, orgID, ownerID string, now time.Time) error {
	if g.OrgID != orgID || (ownerID != "" && g.OwnerID != ownerID) {
		return ErrGrantNotFound
	}
	if g.Revoked {
		return ErrGrantRevoked
	}
	if now.After(g.ExpiresAt) {
		return ErrGrantExpired
	}
	return nil
}

func (s *PgStore) inTx(ctx context.Context, op string, fn func(pgx.Tx) error) error {
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return s.dbErr(ctx, op, err)
	}
	defer func() { _ = tx.Rollback(context.WithoutCancel(ctx)) }()
	if err := fn(tx); err != nil {
		return err
	}
	if err := tx.Commit(ctx); err != nil {
		return s.dbErr(ctx, op, err)
	}
	return nil
}

func (s *PgStore) advisoryLock(ctx context.Context, tx pgx.Tx, op string, ns int32, key string) error {
	if _, err := tx.Exec(ctx, `SELECT pg_advisory_xact_lock($1::int4, hashtext($2))`, ns, key); err != nil {
		return s.dbErr(ctx, op, err)
	}
	return nil
}

func nonNil(values []string) []string {
	if values == nil {
		return []string{}
	}
	return values
}

const insertGrant = `INSERT INTO browser_grants (id, kind, org_id, owner_id, session_key, scope_url,
	allowed_domains, allowed_actions, allowed_frame_ids, allowed_dialog_ids, allowed_artifact_ids,
	parent_grant_id, expires_at, absolute_expires_at)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, NULLIF($12, ''), $13, $13)`

func insertArgs(g *Grant) []any {
	return []any{
		g.ID, string(g.Kind), g.OrgID, g.OwnerID, g.SessionKey, g.ScopeURL,
		nonNil(g.AllowedDomains), nonNil(g.AllowedActions), nonNil(g.AllowedFrameIDs),
		nonNil(g.AllowedDialogIDs), nonNil(g.AllowedArtifactIDs), g.ParentGrantID, g.ExpiresAt,
	}
}

// CreateSession persists a non-ZDR session grant. With MaxActivePerOwner set,
// the count and the insert run under a per-owner advisory lock, so the cap is
// exact under concurrent creates.
func (s *PgStore) CreateSession(ctx context.Context, p CreateSessionParams) (*Grant, error) {
	if p.Zdr {
		return nil, ErrZdrNotPersistable
	}
	domains, err := NormalizeAllowedDomains(p.AllowedDomains)
	if err != nil {
		return nil, err
	}
	id, err := newID(false)
	if err != nil {
		return nil, err
	}
	now := s.clock()
	g := &Grant{
		ID: id, Kind: KindSession, OrgID: p.OrgID, OwnerID: p.OwnerID, SessionKey: p.SessionKey,
		ScopeURL: p.ScopeURL, AllowedDomains: domains, ExpiresAt: now.Add(p.TTL),
	}
	err = s.inTx(ctx, "create_session", func(tx pgx.Tx) error {
		if p.MaxActivePerOwner > 0 {
			if err := s.advisoryLock(ctx, tx, "create_session", lockNsOwnerSessions, p.OrgID+":"+p.OwnerID); err != nil {
				return err
			}
			var active int
			if err := tx.QueryRow(ctx, `SELECT count(*) FROM browser_grants
				WHERE org_id = $1 AND owner_id = $2 AND kind = 'session'
				  AND revoked_at IS NULL AND expires_at >= $3`,
				p.OrgID, p.OwnerID, now).Scan(&active); err != nil {
				return s.dbErr(ctx, "create_session", err)
			}
			if active >= p.MaxActivePerOwner {
				return ErrTooManyGrants
			}
		}
		if _, err := tx.Exec(ctx, insertGrant, insertArgs(g)...); err != nil {
			return s.dbErr(ctx, "create_session", err)
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	return clone(g), nil
}

// CreateScopedSensitiveApproval persists a one-time approval under a session
// or run parent, with the same rules as MemoryRepository. The ancestor chain is
// locked FOR SHARE top-down (root session first, then the parent), so a revoke
// of any ancestor either finishes first (the approval then fails) or waits and
// then cascades to the approval; and the lock order cannot cycle with a
// revoke, which locks its root and then walks down.
//
// A ZDR approval is refused (ErrZdrPostureChanged): this store has no ZDR
// column, and an in-memory approval under a persistent parent would escape the
// revoke cascade. A ZDR caller's approvals live under its ZDR session.
func (s *PgStore) CreateScopedSensitiveApproval(ctx context.Context, p ApprovalParams) (*Grant, error) {
	if strings.HasPrefix(p.ParentGrantID, zdrIDPrefix) {
		return nil, ErrGrantNotFound
	}
	if p.Zdr {
		return nil, ErrZdrPostureChanged
	}
	var out *Grant
	err := s.inTx(ctx, "create_approval", func(tx pgx.Tx) error {
		first, err := s.fetch(ctx, tx, "create_approval", p.ParentGrantID, "")
		if err != nil {
			return err
		}
		if err := scoped(first, p.OrgID, p.OwnerID, s.clock()); err != nil {
			return err
		}
		if first.ParentGrantID != "" {
			root, err := s.fetch(ctx, tx, "create_approval", first.ParentGrantID, " FOR SHARE")
			if err != nil {
				return err
			}
			if err := scoped(root, p.OrgID, p.OwnerID, s.clock()); err != nil {
				return err
			}
		}
		parent, err := s.fetch(ctx, tx, "create_approval", p.ParentGrantID, " FOR SHARE")
		if err != nil {
			return err
		}
		now := s.clock()
		if err := scoped(parent, p.OrgID, p.OwnerID, now); err != nil {
			return err
		}
		if _, ok := approvalParents[parent.Kind]; !ok || parent.SessionKey != p.SessionKey || len(parent.AllowedActions) != 0 {
			return errors.New("sensitive approval parent grant is not an ordinary matching session or run grant")
		}
		domains, err := NormalizeAllowedDomains(p.AllowedDomains)
		if err != nil {
			return err
		}
		for _, host := range domains {
			if !hostWithin(host, parent.AllowedDomains) {
				return ErrDeriveWidensParent
			}
		}
		scopes, err := NormalizeSensitiveScopes(
			p.AllowedActions, p.AllowedFrameIDs, p.AllowedDialogIDs, p.AllowedArtifactIDs,
		)
		if err != nil {
			return err
		}
		if len(scopes.actions) == 0 {
			return errors.New("sensitive grant requires an action")
		}
		id, err := newID(false)
		if err != nil {
			return err
		}
		expires := now.Add(p.TTL)
		if parent.ExpiresAt.Before(expires) {
			expires = parent.ExpiresAt
		}
		g := &Grant{
			ID: id, Kind: KindSensitiveApproval, OrgID: p.OrgID, OwnerID: p.OwnerID,
			SessionKey: p.SessionKey, ScopeURL: p.ScopeURL, AllowedDomains: domains,
			AllowedActions: scopes.actions, AllowedFrameIDs: scopes.frames,
			AllowedDialogIDs: scopes.dialogs, AllowedArtifactIDs: scopes.artifacts,
			ParentGrantID: p.ParentGrantID, ExpiresAt: expires,
		}
		if _, err := tx.Exec(ctx, insertGrant, insertArgs(g)...); err != nil {
			return s.dbErr(ctx, "create_approval", err)
		}
		out = g
		return nil
	})
	if err != nil {
		return nil, err
	}
	return clone(out), nil
}

// Derive issues a run grant with the rules of MemoryRepository.Derive, in one
// transaction:
//
//  1. the parent is read FOR SHARE, so a concurrent RevokeScoped (which locks
//     it FOR UPDATE first) either completed before (the parent reads revoked)
//     or waits for this transaction and then cascades to the new child;
//  2. a per-parent advisory lock serialises derives under one parent, so the
//     live-run count and the insert are exact (DefaultMaxActiveRunsPerParent);
//  3. the insert is ON CONFLICT (parent_grant_id, session_key) DO NOTHING; a
//     replay returns the issued grant unchanged.
func (s *PgStore) Derive(ctx context.Context, p DeriveParams) (*Grant, error) {
	if strings.TrimSpace(p.SessionKey) == "" || p.ParentGrantID == "" || p.OrgID == "" {
		return nil, ErrParentNotDerivable
	}
	if strings.HasPrefix(p.ParentGrantID, zdrIDPrefix) {
		return nil, ErrGrantNotFound
	}
	var out *Grant
	err := s.inTx(ctx, "derive", func(tx pgx.Tx) error {
		parent, err := s.fetch(ctx, tx, "derive", p.ParentGrantID, " FOR SHARE")
		if err != nil {
			return err
		}
		if err := scoped(parent, p.OrgID, p.OwnerID, s.clock()); err != nil {
			return err
		}
		if s.afterParentLock != nil {
			s.afterParentLock()
		}
		policy, ok := derivableParents[parent.Kind]
		if !ok || len(parent.AllowedActions) != 0 {
			return ErrParentNotDerivable
		}
		// A persisted parent is never ZDR, so a ZDR caller would obtain a
		// grant whose run may be retained.
		if p.PrincipalZdr {
			return ErrZdrPostureChanged
		}
		maxRun := p.MaxRunDomains
		if maxRun <= 0 {
			maxRun = DefaultMaxRunDomains
		}
		domains, err := deriveDomains(parent.AllowedDomains, p.RunDomains, maxRun, policy.explicitHosts)
		if err != nil {
			return err
		}
		if p.Policy != nil {
			if err := p.Policy.CheckSession(ctx, p.OrgID, domains); err != nil {
				return err
			}
		}
		if err := s.advisoryLock(ctx, tx, "derive", lockNsParentRuns, parent.ID); err != nil {
			return err
		}
		now := s.clock()
		// The parent may have expired while this transaction waited.
		if now.After(parent.ExpiresAt) {
			return ErrGrantExpired
		}
		replay := func() error {
			existing, err := s.existingRun(ctx, tx, parent.ID, parent.OrgID, p.SessionKey)
			if err != nil || existing == nil {
				return err
			}
			switch {
			case existing.Revoked:
				return ErrGrantRevoked
			case now.After(existing.ExpiresAt):
				return ErrGrantExpired
			case !equalStrings(existing.AllowedDomains, domains):
				return ErrDeriveConflict
			}
			out = existing
			return nil
		}
		if err := replay(); err != nil || out != nil {
			return err
		}
		expires := now.Add(p.TTL)
		if parent.ExpiresAt.Before(expires) {
			expires = parent.ExpiresAt
		}
		if expires.Sub(now) < MinDerivedLifetime {
			return ErrParentNotDerivable
		}
		maxRuns := p.MaxActivePerParent
		if maxRuns <= 0 {
			maxRuns = DefaultMaxActiveRunsPerParent
		}
		var active int
		if err := tx.QueryRow(ctx, `SELECT count(*) FROM browser_grants
			WHERE parent_grant_id = $1 AND org_id = $2 AND kind = 'run'
			  AND revoked_at IS NULL AND expires_at >= $3`,
			parent.ID, parent.OrgID, now).Scan(&active); err != nil {
			return s.dbErr(ctx, "derive", err)
		}
		if active >= maxRuns {
			return ErrTooManyGrants
		}
		id, err := newID(false)
		if err != nil {
			return err
		}
		g := &Grant{
			ID: id, Kind: KindRun, OrgID: parent.OrgID, OwnerID: parent.OwnerID,
			SessionKey: p.SessionKey, ScopeURL: parent.ScopeURL, AllowedDomains: domains,
			ParentGrantID: parent.ID, ExpiresAt: expires,
		}
		var inserted string
		err = tx.QueryRow(ctx, insertGrant+`
			ON CONFLICT (parent_grant_id, session_key) WHERE kind = 'run' DO NOTHING
			RETURNING id`, insertArgs(g)...).Scan(&inserted)
		if errors.Is(err, pgx.ErrNoRows) {
			// Another derive with the same key won; the advisory lock makes
			// this unreachable, but the replay rules still hold if it is not.
			if err := replay(); err != nil {
				return err
			}
			if out == nil {
				return ErrDeriveConflict
			}
			return nil
		}
		if err != nil {
			return s.dbErr(ctx, "derive", err)
		}
		out = g
		return nil
	})
	if err != nil {
		return nil, err
	}
	return clone(out), nil
}

func (s *PgStore) existingRun(ctx context.Context, q querier, parentID, orgID, sessionKey string) (*Grant, error) {
	var id string
	err := q.QueryRow(ctx, `SELECT id FROM browser_grants
		WHERE parent_grant_id = $1 AND session_key = $2 AND kind = 'run' AND org_id = $3`,
		parentID, sessionKey, orgID).Scan(&id)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, s.dbErr(ctx, "derive", err)
	}
	return s.fetch(ctx, q, "derive", id, "")
}

// GetScoped returns a grant only within the verified organization and, with a
// non-empty ownerID, the owning user. A "z_" id never reaches the database.
func (s *PgStore) GetScoped(ctx context.Context, id, orgID, ownerID string) (*Grant, error) {
	if strings.HasPrefix(id, zdrIDPrefix) {
		return nil, ErrGrantNotFound
	}
	g, err := s.fetch(ctx, s.pool, "get", id, "")
	if err != nil {
		return nil, err
	}
	if err := scoped(g, orgID, ownerID, s.clock()); err != nil {
		return nil, err
	}
	return g, nil
}

// revokeSubtree revokes a grant and every grant derived from it, at any depth,
// in one statement.
const revokeSubtree = `WITH RECURSIVE subtree(id) AS (
	SELECT id FROM browser_grants WHERE id = $1 AND org_id = $2
	UNION
	SELECT c.id FROM browser_grants c JOIN subtree t ON c.parent_grant_id = t.id
	WHERE c.org_id = $2
)
UPDATE browser_grants g
SET revoked_at = COALESCE(g.revoked_at, $3), revoked_reason = $4
FROM subtree WHERE g.id = subtree.id`

// RevokeScoped revokes the grant and its whole subtree. The root is locked FOR
// UPDATE first, which waits for every in-flight Derive or approval holding it
// FOR SHARE; the recursive update then runs as a new statement, so its
// snapshot includes any child those transactions committed. (As a single
// statement, the subtree would be computed from a snapshot taken before the
// lock wait and could miss that child.)
func (s *PgStore) RevokeScoped(ctx context.Context, id, orgID, ownerID, reason string) error {
	if strings.HasPrefix(id, zdrIDPrefix) {
		return ErrGrantNotFound
	}
	reason = truncateReason(reason)
	return s.inTx(ctx, "revoke", func(tx pgx.Tx) error {
		g, err := s.fetch(ctx, tx, "revoke", id, " FOR UPDATE")
		if err != nil {
			return err
		}
		if g.OrgID != orgID || (ownerID != "" && g.OwnerID != ownerID) {
			return ErrGrantNotFound
		}
		if s.afterRootLock != nil {
			s.afterRootLock()
		}
		if _, err := tx.Exec(ctx, revokeSubtree, id, orgID, s.clock(), reason); err != nil {
			return s.dbErr(ctx, "revoke", err)
		}
		return nil
	})
}

// PurgeExpired deletes grants that expired before the cutoff; a deleted
// parent takes its subtree with it (ON DELETE CASCADE). It returns the number
// of rows the statement matched, not counting cascaded children.
func (s *PgStore) PurgeExpired(ctx context.Context, before time.Time) (int64, error) {
	tag, err := s.pool.Exec(ctx, `DELETE FROM browser_grants WHERE expires_at < $1`, before.UTC())
	if err != nil {
		return 0, s.dbErr(ctx, "purge", err)
	}
	return tag.RowsAffected(), nil
}
