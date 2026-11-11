package grant

import (
	"context"
	"errors"
	"log/slog"
	"strings"
	"sync/atomic"
	"time"
	"unicode/utf8"

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
	// ErrInvalidGrantText means a caller string cannot be stored (a NUL byte
	// or invalid UTF-8): PostgreSQL text rejects both, so the request is
	// refused as invalid input instead of failing as a store outage.
	ErrInvalidGrantText = errors.New("grant field contains a NUL byte or invalid UTF-8")
	// ErrZdrNotPersistable means a Zero Data Retention grant reached the
	// durable store. Routed never sends one; the store refuses it anyway.
	ErrZdrNotPersistable = errors.New("zero data retention grant cannot be persisted")
)

// Advisory lock namespaces (the first key of pg_advisory_xact_lock(int, int)),
// so the per-owner and per-parent locks never share a key.
const (
	lockNsOwnerSessions int32 = 0x62620001
	lockNsParentRuns    int32 = 0x62620002
	lockNsRevokeRoot    int32 = 0x62620003
	lockNsKeyGrants     int32 = 0x62620004
	lockNsParentExts    int32 = 0x62620005
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
	_ KeyRevoker = (*PgStore)(nil)
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

// storeError is ErrStoreUnavailable plus the SQLSTATE of the failure, kept
// only so inTx can retry a deadlock or serialization failure. Its text and
// errors.Is behaviour are exactly ErrStoreUnavailable's.
type storeError struct{ sqlState string }

func (e *storeError) Error() string        { return ErrStoreUnavailable.Error() }
func (e *storeError) Is(target error) bool { return target == ErrStoreUnavailable }

// retryableTx reports a failure that PostgreSQL resolved by aborting this
// transaction (deadlock victim, serialization failure): nothing was written,
// so running the whole transaction again is safe.
func retryableTx(err error) bool {
	var se *storeError
	return errors.As(err, &se) && (se.sqlState == "40P01" || se.sqlState == "40001")
}

// txAttempts bounds how many times inTx runs a transaction that PostgreSQL
// aborted as a deadlock or serialization victim.
const txAttempts = 3

// deadlockRetries counts transactions inTx ran again after a 40P01 deadlock
// abort. It exists so a test can prove the advisory locks keep concurrent
// revokes from deadlocking at all (a retry alone would hide it).
var deadlockRetries atomic.Int64

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
	return &storeError{sqlState: sqlState}
}

// validText reports whether every string can be stored in a PostgreSQL text
// column.
func validText(values ...string) bool {
	for _, v := range values {
		if !utf8.ValidString(v) || strings.IndexByte(v, 0) >= 0 {
			return false
		}
	}
	return true
}

type querier interface {
	QueryRow(ctx context.Context, sql string, args ...any) pgx.Row
	Exec(ctx context.Context, sql string, args ...any) (pgconn.CommandTag, error)
}

const grantColumns = `id, kind, org_id, owner_id, session_key, scope_url, allowed_domains,
	allowed_actions, allowed_frame_ids, allowed_dialog_ids, allowed_artifact_ids,
	COALESCE(parent_grant_id, ''), expires_at, revoked_at IS NOT NULL, COALESCE(revoked_reason, ''),
	COALESCE(budget_micro_usd, 0), COALESCE(budget_max_steps, 0), COALESCE(budget_max_runtime_s, 0),
	COALESCE(budget_source, ''), COALESCE(budget_minted_at_unix, 0),
	policy_mode, denied_domains, COALESCE(resource_id, ''), absolute_expires_at, COALESCE(key_id, ''),
	COALESCE(consent_scope, '')`

// fetch reads one grant; lock is "", " FOR SHARE" or " FOR UPDATE". A
// missing row is ErrGrantNotFound.
func (s *PgStore) fetch(ctx context.Context, q querier, op, id, lock string) (*Grant, error) {
	var g Grant
	var kind string
	err := q.QueryRow(ctx, `SELECT `+grantColumns+` FROM browser_grants WHERE id = $1`+lock, id).Scan(
		&g.ID, &kind, &g.OrgID, &g.OwnerID, &g.SessionKey, &g.ScopeURL, &g.AllowedDomains,
		&g.AllowedActions, &g.AllowedFrameIDs, &g.AllowedDialogIDs, &g.AllowedArtifactIDs,
		&g.ParentGrantID, &g.ExpiresAt, &g.Revoked, &g.RevokedReason,
		&g.Budget.MicroUSD, &g.Budget.MaxSteps, &g.Budget.MaxRuntimeS, &g.Budget.Source, &g.Budget.MintedAtUnix,
		&g.PolicyMode, &g.DeniedDomains, &g.ResourceID, &g.AbsoluteExpiresAt, &g.KeyID,
		&g.ConsentScope,
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

// inTx runs fn in a transaction, and runs it again (at most txAttempts times
// in all) when PostgreSQL aborted it as a deadlock or serialization victim.
// fn must therefore be safe to repeat: every one here only reads and writes
// through tx, and a retry starts from a rolled-back transaction.
func (s *PgStore) inTx(ctx context.Context, op string, fn func(pgx.Tx) error) error {
	var err error
	for attempt := 1; attempt <= txAttempts; attempt++ {
		if err = s.inTxOnce(ctx, op, fn); !retryableTx(err) {
			return err
		}
		var se *storeError
		if errors.As(err, &se) && se.sqlState == "40P01" {
			deadlockRetries.Add(1)
		}
		select {
		case <-ctx.Done():
			return err
		case <-time.After(time.Duration(attempt) * 5 * time.Millisecond):
		}
	}
	return err
}

func (s *PgStore) inTxOnce(ctx context.Context, op string, fn func(pgx.Tx) error) error {
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
	parent_grant_id, expires_at, absolute_expires_at,
	budget_micro_usd, budget_max_steps, budget_max_runtime_s, budget_source, budget_minted_at_unix,
	policy_mode, denied_domains, resource_id, key_id, consent_scope)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, NULLIF($12, ''), $13, $19,
	NULLIF($14::bigint, 0), NULLIF($15::int, 0), NULLIF($16::int, 0), NULLIF($17::text, ''), NULLIF($18::bigint, 0),
	$20, $21, NULLIF($22::text, ''), NULLIF($23::text, ''), NULLIF($24::text, ''))`

func insertArgs(g *Grant) []any {
	absolute := g.AbsoluteExpiresAt
	if absolute.IsZero() {
		absolute = g.ExpiresAt
	}
	mode := g.PolicyMode
	if mode == "" {
		mode = PolicyModeAllowlist
	}
	return []any{
		g.ID, string(g.Kind), g.OrgID, g.OwnerID, g.SessionKey, g.ScopeURL,
		nonNil(g.AllowedDomains), nonNil(g.AllowedActions), nonNil(g.AllowedFrameIDs),
		nonNil(g.AllowedDialogIDs), nonNil(g.AllowedArtifactIDs), g.ParentGrantID, g.ExpiresAt,
		g.Budget.MicroUSD, g.Budget.MaxSteps, g.Budget.MaxRuntimeS, g.Budget.Source, g.Budget.MintedAtUnix,
		absolute, mode, nonNil(g.DeniedDomains), g.ResourceID, g.KeyID, g.ConsentScope,
	}
}

// CreateSession persists a non-ZDR session grant. With MaxActivePerOwner set,
// the count and the insert run under a per-owner advisory lock, so the cap is
// exact under concurrent creates.
func (s *PgStore) CreateSession(ctx context.Context, p CreateSessionParams) (*Grant, error) {
	if p.Zdr {
		return nil, ErrZdrNotPersistable
	}
	if !validText(p.OrgID, p.OwnerID, p.SessionKey, p.ScopeURL) {
		return nil, ErrInvalidGrantText
	}
	if p.KeyID != "" && !ValidKeyID(p.KeyID) {
		return nil, ErrInvalidKeyID
	}
	domains, err := NormalizeAllowedDomains(p.AllowedDomains)
	if err != nil {
		return nil, err
	}
	budget, err := NormalizeBudget(p.Budget)
	if err != nil {
		return nil, err
	}
	id, err := newID(false)
	if err != nil {
		return nil, err
	}
	now := s.clock()
	if budget.IsSet() {
		budget.MintedAtUnix = now.Unix()
	}
	g := &Grant{
		ID: id, Kind: KindSession, OrgID: p.OrgID, OwnerID: p.OwnerID, SessionKey: p.SessionKey,
		ScopeURL: p.ScopeURL, AllowedDomains: domains, Budget: budget, ExpiresAt: now.Add(p.TTL),
		PolicyMode: PolicyModeAllowlist, KeyID: p.KeyID,
	}
	g.AbsoluteExpiresAt = g.ExpiresAt
	err = s.inTx(ctx, "create_session", func(tx pgx.Tx) error {
		if p.MaxActivePerOwner > 0 {
			if err := s.advisoryLock(ctx, tx, "create_session", lockNsOwnerSessions, p.OrgID+":"+p.OwnerID); err != nil {
				return err
			}
			var active int
			if err := tx.QueryRow(ctx, `SELECT count(*) FROM browser_grants
				WHERE org_id = $1 AND owner_id = $2 AND kind IN ('session', 'human_browse')
				  AND revoked_at IS NULL AND expires_at >= $3`,
				p.OrgID, p.OwnerID, now).Scan(&active); err != nil {
				return s.dbErr(ctx, "create_session", err)
			}
			if active >= p.MaxActivePerOwner {
				return ErrTooManyGrants
			}
		}
		if p.KeyID != "" {
			// After the owner lock (always that order), so the per-key count
			// and the insert are exact, and a concurrent RevokeByKey (which
			// takes this lock) either sees this grant or runs before it.
			if err := s.advisoryLock(ctx, tx, "create_session", lockNsKeyGrants, p.KeyID); err != nil {
				return err
			}
			var active int
			if err := tx.QueryRow(ctx, `SELECT count(*) FROM browser_grants
				WHERE org_id = $1 AND key_id = $2 AND revoked_at IS NULL AND expires_at >= $3`,
				p.OrgID, p.KeyID, now).Scan(&active); err != nil {
				return s.dbErr(ctx, "create_session", err)
			}
			if active >= maxPerKey(p.MaxActivePerKey) {
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
	// Q3-E3.1-b1-c: this store has no columns for a fill_otp binding, so a
	// validated grant would come back without its origin and mail connection.
	if IsFillOtp(p.AllowedActions) {
		return nil, ErrFillOtpNotDurable
	}
	if p.GrantOrigin != "" || p.MailConnectionID != "" {
		return nil, ErrFillOtpBindingUnexpected
	}
	if !validText(p.OrgID, p.OwnerID, p.SessionKey, p.ScopeURL, p.ParentGrantID) {
		return nil, ErrInvalidGrantText
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
		if _, ok := approvalParents[parent.Kind]; !ok || parent.SessionKey != p.SessionKey || len(parent.AllowedActions) != 0 ||
			parent.KeyID != "" {
			return ErrApprovalParentInvalid
		}
		domains, err := NormalizeAllowedDomains(p.AllowedDomains)
		if err != nil {
			return err
		}
		for _, host := range domains {
			if !hostWithinGrant(host, parent) {
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
			ParentGrantID: p.ParentGrantID, ExpiresAt: expires, AbsoluteExpiresAt: expires,
			PolicyMode: PolicyModeAllowlist,
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
//     replay returns the issued grant unchanged (see sameDeriveBudget for
//     what a replay may say about the budget claim).
func (s *PgStore) Derive(ctx context.Context, p DeriveParams) (*Grant, error) {
	if strings.TrimSpace(p.SessionKey) == "" || p.ParentGrantID == "" || p.OrgID == "" {
		return nil, ErrParentNotDerivable
	}
	if strings.HasPrefix(p.ParentGrantID, zdrIDPrefix) {
		return nil, ErrGrantNotFound
	}
	if !validText(p.OrgID, p.OwnerID, p.SessionKey, p.ParentGrantID) {
		return nil, ErrInvalidGrantText
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
		plan, err := planDerive(ctx, p, parent)
		if err != nil {
			return err
		}
		if err := s.advisoryLock(ctx, tx, "derive", lockNsParentRuns, parent.ID); err != nil {
			return err
		}
		now := s.clock()
		// The parent may have expired while this transaction waited.
		if now.After(parent.ExpiresAt) {
			return ErrGrantExpired
		}
		budget, err := tightenBudget(parent.Budget, plan.requestedBudget, now.Unix())
		if err != nil {
			return err
		}
		replay := func() error {
			existing, err := s.existingChild(ctx, tx, plan.kind, parent.ID, parent.OrgID, p.SessionKey)
			if err != nil || existing == nil {
				return err
			}
			switch {
			case existing.Revoked:
				return ErrGrantRevoked
			case now.After(existing.ExpiresAt):
				return ErrGrantExpired
			case !equalStrings(existing.AllowedDomains, plan.domains), existing.ResourceID != plan.resourceID,
				!sameDeriveBudget(plan.requestedBudget, existing.Budget, budget):
				return ErrDeriveConflict
			}
			out = existing
			return nil
		}
		if err := replay(); err != nil || out != nil {
			return err
		}
		expires := now.Add(plan.ttl)
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
			WHERE parent_grant_id = $1 AND org_id = $2 AND kind = $4
				  AND revoked_at IS NULL AND expires_at >= $3`,
			parent.ID, parent.OrgID, now, string(plan.kind)).Scan(&active); err != nil {
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
			ID: id, Kind: plan.kind, OrgID: parent.OrgID, OwnerID: parent.OwnerID,
			SessionKey: p.SessionKey, ScopeURL: parent.ScopeURL, AllowedDomains: plan.domains,
			Budget: budget, ParentGrantID: parent.ID, ExpiresAt: expires, AbsoluteExpiresAt: expires,
			PolicyMode: PolicyModeAllowlist, ResourceID: plan.resourceID,
		}
		var inserted string
		// The idempotency index is partial per kind: 'run' (0001) and 'attach' (0004).
		err = tx.QueryRow(ctx, insertGrant+`
				ON CONFLICT (parent_grant_id, session_key) WHERE kind = '`+string(plan.kind)+`' DO NOTHING
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

// existingChild returns the run or attach grant already derived from parentID
// under sessionKey, or nil.
func (s *PgStore) existingChild(ctx context.Context, q querier, kind Kind, parentID, orgID, sessionKey string) (*Grant, error) {
	var id string
	err := q.QueryRow(ctx, `SELECT id FROM browser_grants
		WHERE parent_grant_id = $1 AND session_key = $2 AND kind = $4 AND org_id = $3`,
		parentID, sessionKey, orgID, string(kind)).Scan(&id)
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
	if strings.HasPrefix(id, zdrIDPrefix) || !validText(id, orgID, ownerID) {
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

// maxGrantDepth is the longest parent chain (session, run, approval), with
// one spare level.
const maxGrantDepth = 4

// revokeRootID follows parent_grant_id up from id to the topmost grant. The
// link is written once at insert and never changes, so the unlocked read is
// stable. A missing row ends the walk at the id it reached.
func (s *PgStore) revokeRootID(ctx context.Context, tx pgx.Tx, id string) (string, error) {
	for i := 0; i < maxGrantDepth; i++ {
		var parent string
		err := tx.QueryRow(ctx, `SELECT COALESCE(parent_grant_id, '') FROM browser_grants WHERE id = $1`, id).Scan(&parent)
		if errors.Is(err, pgx.ErrNoRows) || (err == nil && parent == "") {
			return id, nil
		}
		if err != nil {
			return "", s.dbErr(ctx, "revoke", err)
		}
		id = parent
	}
	return id, nil
}

// RevokeScoped revokes the grant and its whole subtree. Revokes that share a
// root session are serialized by an advisory lock on that root: two revokes
// (of a session and of one of its runs, say) lock overlapping rows in
// different orders, and PostgreSQL aborted one as a deadlock victim, leaving a
// revoked parent live until the caller retried. inTx also retries a
// deadlock, as a second line. The root is then locked FOR
// UPDATE, which waits for every in-flight Derive or approval holding it
// FOR SHARE; the recursive update then runs as a new statement, so its
// snapshot includes any child those transactions committed. (As a single
// statement, the subtree would be computed from a snapshot taken before the
// lock wait and could miss that child.)
func (s *PgStore) RevokeScoped(ctx context.Context, id, orgID, ownerID, reason string) error {
	if strings.HasPrefix(id, zdrIDPrefix) {
		return ErrGrantNotFound
	}
	reason = truncateReason(reason)
	if !validText(id, orgID, ownerID) {
		return ErrGrantNotFound
	}
	return s.inTx(ctx, "revoke", func(tx pgx.Tx) error {
		root, err := s.revokeRootID(ctx, tx, id)
		if err != nil {
			return err
		}
		if err := s.advisoryLock(ctx, tx, "revoke", lockNsRevokeRoot, root); err != nil {
			return err
		}
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

// revokeKeySubtree revokes every not-yet-revoked grant of one API key and
// anything derived from one (none should exist), in one statement.
const revokeKeySubtree = `WITH RECURSIVE subtree(id) AS (
	SELECT id FROM browser_grants WHERE key_id = $1
	UNION
	SELECT c.id FROM browser_grants c JOIN subtree t ON c.parent_grant_id = t.id
)
UPDATE browser_grants g
SET revoked_at = $2, revoked_reason = $3
FROM subtree WHERE g.id = subtree.id AND g.revoked_at IS NULL`

// RevokeByKey revokes every live grant issued for keyID (INNH-10, the
// api_key_revoked consumer), in every org. It takes the per-key advisory lock
// CreateSession takes for a key grant, so a key grant being created
// concurrently either commits first (and is revoked here) or waits and is
// created after; the broker's revoked-key list refuses that second case.
func (s *PgStore) RevokeByKey(ctx context.Context, keyID, reason string) (int64, error) {
	if !ValidKeyID(keyID) {
		return 0, ErrInvalidKeyID
	}
	reason = truncateReason(reason)
	var n int64
	err := s.inTx(ctx, "revoke_key", func(tx pgx.Tx) error {
		if err := s.advisoryLock(ctx, tx, "revoke_key", lockNsKeyGrants, keyID); err != nil {
			return err
		}
		tag, err := tx.Exec(ctx, revokeKeySubtree, keyID, s.clock(), reason)
		if err != nil {
			return s.dbErr(ctx, "revoke_key", err)
		}
		n = tag.RowsAffected()
		return nil
	})
	if err != nil {
		return 0, err
	}
	return n, nil
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
