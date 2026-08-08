// Package orgscope runs a database transaction under Data Plane v2's
// row-level-security org isolation (Phase 1 RLS).
//
// # What this is for
//
// Every DPv2 service connects to Postgres as `dataplane`, which is SUPERUSER
// and owns the tables — and PostgreSQL superusers bypass RLS unconditionally.
// So the policies added by `20260809120000_org_rls_isolation.sql` do nothing on
// a normal connection; they only apply inside a transaction that has explicitly
// dropped to the restricted RuntimeRole. That is what WithOrgScope does:
//
//	SELECT set_config('app.current_org', $1, true);  -- SET LOCAL, parameterized
//	SET LOCAL ROLE dataplane_app;                    -- drop superuser
//
// Both are transaction-local, so COMMIT/ROLLBACK reverts them and a pooled
// connection is never left de-privileged or carrying another request's org.
//
// This is the Go counterpart of the Rust `pg-org-scope-rs` crate and a direct
// port of Control Plane org-core's own DB.WithOrgScope
// (internal/database/database.go), which has been audited and empirically
// verified against its live database.
//
// # When NOT to use this
//
// Some work is legitimately cross-org and must keep running on the normal
// (unscoped, superuser) pool — the same documented exceptions org-core's audit
// identified, and which DPv2 has more of than you might expect:
//
//   - Background workers draining a queue or outbox for every org. A scoped
//     transaction sees one org only, so wrapping a poller silently stops it
//     draining everyone else's work. DPv2's embedding batch pipeline is
//     explicitly like this — it mixes orgs in one buffer by design.
//   - GDPR/erasure and admin rebuild paths that operate across orgs, including
//     the rows they write with a NULL org_id (admin_audit_log,
//     quickwit_admin_jobs, quickwit_admin_job_audit). A NULL org_id never
//     satisfies the policy, so a scoped INSERT of one is rejected by WITH
//     CHECK — correctly, since such a row belongs to no tenant.
//
// Use this for request-scoped paths acting on behalf of exactly one org.
// Classify the call site before wrapping it.
package orgscope

import (
	"context"
	"errors"
	"fmt"
	"strings"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// RuntimeRole is the NOLOGIN, NOBYPASSRLS role a scoped transaction switches
// to, created by 20260809120000_org_rls_isolation.sql.
//
// Hardcoded rather than configurable: it is interpolated into SET LOCAL ROLE,
// which cannot take a bind parameter, so a caller-supplied value would be an
// injection vector for no practical benefit.
const RuntimeRole = "dataplane_app"

// OrgGUC is the transaction-local setting the RLS policies compare org_id
// against. It must match the migration's policy expression exactly; a drift
// here silently disables every policy.
const OrgGUC = "app.current_org"

// ErrEmptyOrgID is returned when no organization is supplied. Setting the GUC
// to "" would match no row, surfacing later as a confusing "everything is
// empty" bug rather than an obvious error, so it is rejected up front.
var ErrEmptyOrgID = errors.New("orgscope: a non-empty orgID is required")

// WithOrgScope runs fn inside a transaction pinned to orgID and enforced by
// row-level security.
//
// The transaction is committed when fn returns nil and rolled back otherwise.
// A rollback is also attempted on every path that does not reach the commit,
// so an early return or panic cannot leak an open transaction.
func WithOrgScope(ctx context.Context, pool *pgxpool.Pool, orgID string, fn func(pgx.Tx) error) error {
	if strings.TrimSpace(orgID) == "" {
		return ErrEmptyOrgID
	}

	tx, err := pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("orgscope: begin: %w", err)
	}
	// No-op after a successful Commit; the safety net for every other path.
	defer func() { _ = tx.Rollback(ctx) }()

	// set_config(..., is_local => true) is exactly SET LOCAL, but
	// parameterized — SET LOCAL itself cannot bind. This is what keeps a
	// hostile orgID from being SQL injection.
	if _, err := tx.Exec(ctx, "SELECT set_config($1, $2, true)", OrgGUC, orgID); err != nil {
		return fmt.Errorf("orgscope: set org scope: %w", err)
	}

	// Order matters: set the GUC first, then drop privilege. The reverse works
	// today but leaves a window where the role is restricted and the org is
	// unset, which is a strictly worse failure mode to debug.
	if _, err := tx.Exec(ctx, "SET LOCAL ROLE "+RuntimeRole); err != nil {
		return fmt.Errorf("orgscope: set runtime role: %w", err)
	}

	if err := fn(tx); err != nil {
		return err
	}

	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("orgscope: commit: %w", err)
	}
	return nil
}
