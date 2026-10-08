package store

import (
	"context"
	"errors"
	"fmt"
	"sort"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"

	"github.com/triodelab/integration-corev2/internal/zdr"
)

// PostgreSQL implementation of ConnectionZDRStore (migration 0013_model_connection_zdr.sql).
// The in-memory implementation (memory_zdr.go) mirrors it check for check; the shared
// contract suite in internal/store/storetest runs against both.

const connectionZDRColumnList = "connection_id, provider_key, auth_mode, capability, basis, basis_ref, catalog_version, digest, checked_at, created_at, updated_at"

const zdrAcknowledgementColumnList = "id, organization_id, connection_id, user_id, provider_key, capability, basis, catalog_version, digest, text_version, locale, text_sha256, text_snapshot, org_zdr_at_ack, acknowledged_via, request_id, acknowledged_at, revoked_at, revoke_reason"

const (
	pgForeignKeyViolation = "23503"
	pgUniqueViolation     = "23505"
)

// zdrQuerier is the part of a pool or transaction the ZDR statements use.
type zdrQuerier interface {
	Exec(context.Context, string, ...any) (pgconn.CommandTag, error)
	Query(context.Context, string, ...any) (pgx.Rows, error)
	QueryRow(context.Context, string, ...any) pgx.Row
}

// inTransaction runs fn atomically. Inside WithAuditTransaction the repository's pool already
// is the open transaction, and fn simply joins it (so the record, the revocations and the
// caller's audit row commit or roll back together); otherwise it opens and commits its own.
func (r *PostgresRepository) inTransaction(ctx context.Context, fn func(zdrQuerier) error) error {
	if _, joined := r.pool.(postgresTransactionPool); joined {
		return fn(r.pool)
	}
	tx, err := r.pool.BeginTx(ctx, pgx.TxOptions{})
	if err != nil {
		return fmt.Errorf("begin connection zdr transaction: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	if err := fn(tx); err != nil {
		return err
	}
	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("commit connection zdr transaction: %w", err)
	}
	return nil
}

func scanConnectionZDR(row pgx.Row) (ConnectionZDR, error) {
	var record ConnectionZDR
	var capability, basis string
	if err := row.Scan(
		&record.ConnectionID, &record.ProviderKey, &record.AuthMode, &capability, &basis, &record.BasisRef,
		&record.CatalogVersion, &record.Digest, &record.CheckedAt, &record.CreatedAt, &record.UpdatedAt,
	); err != nil {
		return ConnectionZDR{}, err
	}
	record.Capability = zdr.Capability(capability)
	record.Basis = zdr.Basis(basis)
	return record, nil
}

func scanZDRAcknowledgement(row pgx.Row) (ConnectionZDRAcknowledgement, error) {
	var ack ConnectionZDRAcknowledgement
	var capability, basis string
	if err := row.Scan(
		&ack.ID, &ack.OrganizationID, &ack.ConnectionID, &ack.UserID, &ack.ProviderKey, &capability, &basis,
		&ack.CatalogVersion, &ack.Digest, &ack.TextVersion, &ack.Locale, &ack.TextSHA256, &ack.TextSnapshot,
		&ack.OrgZDRAtAck, &ack.AcknowledgedVia, &ack.RequestID, &ack.AcknowledgedAt, &ack.RevokedAt, &ack.RevokeReason,
	); err != nil {
		return ConnectionZDRAcknowledgement{}, err
	}
	ack.Capability = zdr.Capability(capability)
	ack.Basis = zdr.Basis(basis)
	return ack, nil
}

func collectZDRAcknowledgements(rows pgx.Rows) ([]ConnectionZDRAcknowledgement, error) {
	defer rows.Close()
	out := []ConnectionZDRAcknowledgement{}
	for rows.Next() {
		ack, err := scanZDRAcknowledgement(rows)
		if err != nil {
			return nil, fmt.Errorf("scan zdr acknowledgement: %w", err)
		}
		out = append(out, ack)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterate zdr acknowledgements: %w", err)
	}
	sort.SliceStable(out, func(i, j int) bool {
		if !out[i].AcknowledgedAt.Equal(out[j].AcknowledgedAt) {
			return out[i].AcknowledgedAt.After(out[j].AcknowledgedAt)
		}
		return out[i].ID < out[j].ID
	})
	return out, nil
}

func pgErrorCode(err error) string {
	var pgErr *pgconn.PgError
	if errors.As(err, &pgErr) {
		return pgErr.Code
	}
	return ""
}

func (r *PostgresRepository) ApplyConnectionZDRAssessment(ctx context.Context, assessment ConnectionZDRAssessment) (ConnectionZDRApplyResult, error) {
	if err := assessment.Validate(); err != nil {
		return ConnectionZDRApplyResult{}, err
	}
	var result ConnectionZDRApplyResult
	err := r.inTransaction(ctx, func(q zdrQuerier) error {
		next := assessment.record(time.Now().UTC())
		existing, err := scanConnectionZDR(q.QueryRow(ctx,
			`SELECT `+connectionZDRColumnList+` FROM integration_connection_zdr WHERE connection_id = $1 FOR UPDATE`,
			assessment.ConnectionID))
		found := err == nil
		if err != nil && !errors.Is(err, pgx.ErrNoRows) {
			return fmt.Errorf("lock connection zdr record: %w", err)
		}
		if !found {
			inserted, insertErr := scanConnectionZDR(q.QueryRow(ctx, `
				INSERT INTO integration_connection_zdr (`+connectionZDRColumnList+`)
				VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$9,$9)
				ON CONFLICT (connection_id) DO NOTHING
				RETURNING `+connectionZDRColumnList,
				next.ConnectionID, next.ProviderKey, next.AuthMode, string(next.Capability), string(next.Basis), next.BasisRef,
				next.CatalogVersion, next.Digest, next.CheckedAt))
			switch {
			case insertErr == nil:
				result.Record = inserted
				result.Created = true
			case pgErrorCode(insertErr) == pgForeignKeyViolation:
				return ErrNotFound
			case errors.Is(insertErr, pgx.ErrNoRows):
				// Another writer created the record between our lock attempt and our insert;
				// lock theirs and carry on as an update.
				existing, err = scanConnectionZDR(q.QueryRow(ctx,
					`SELECT `+connectionZDRColumnList+` FROM integration_connection_zdr WHERE connection_id = $1 FOR UPDATE`,
					assessment.ConnectionID))
				if err != nil {
					return fmt.Errorf("lock connection zdr record after concurrent insert: %w", err)
				}
				found = true
			default:
				return fmt.Errorf("insert connection zdr record: %w", insertErr)
			}
		}
		if found {
			previous := existing
			if existing.Digest == next.Digest && existing.BasisRef == next.BasisRef {
				result.Record = existing
				result.Previous = &previous
			} else {
				updated, updateErr := scanConnectionZDR(q.QueryRow(ctx, `
					UPDATE integration_connection_zdr
					SET provider_key = $2, auth_mode = $3, capability = $4, basis = $5, basis_ref = $6,
					    catalog_version = $7, digest = $8, checked_at = $9, updated_at = $9
					WHERE connection_id = $1
					RETURNING `+connectionZDRColumnList,
					next.ConnectionID, next.ProviderKey, next.AuthMode, string(next.Capability), string(next.Basis), next.BasisRef,
					next.CatalogVersion, next.Digest, next.CheckedAt))
				if updateErr != nil {
					return fmt.Errorf("update connection zdr record: %w", updateErr)
				}
				result.Record = updated
				result.Previous = &previous
				result.Changed = existing.Digest != next.Digest
			}
		}
		// Every active acknowledgement bound to another digest is stale. This runs on every
		// apply, not only when the digest changed, so a missed revocation heals.
		rows, err := q.Query(ctx, `
			UPDATE integration_connection_zdr_acknowledgements
			SET revoked_at = $3, revoke_reason = $4
			WHERE connection_id = $1 AND revoked_at IS NULL AND digest <> $2
			RETURNING `+zdrAcknowledgementColumnList,
			assessment.ConnectionID, result.Record.Digest, next.CheckedAt, ZDRRevokeCapabilityChanged)
		if err != nil {
			return fmt.Errorf("revoke stale zdr acknowledgements: %w", err)
		}
		revoked, err := collectZDRAcknowledgements(rows)
		if err != nil {
			return err
		}
		result.Revoked = revoked
		return nil
	})
	if err != nil {
		return ConnectionZDRApplyResult{}, err
	}
	if len(result.Revoked) == 0 {
		result.Revoked = nil
	}
	return result, nil
}

func (r *PostgresRepository) GetConnectionZDR(ctx context.Context, connectionID string) (ConnectionZDR, error) {
	record, err := scanConnectionZDR(r.pool.QueryRow(ctx,
		`SELECT `+connectionZDRColumnList+` FROM integration_connection_zdr WHERE connection_id = $1`, connectionID))
	if errors.Is(err, pgx.ErrNoRows) {
		return ConnectionZDR{}, ErrNotFound
	}
	if err != nil {
		return ConnectionZDR{}, fmt.Errorf("get connection zdr record: %w", err)
	}
	return record, nil
}

func (r *PostgresRepository) InsertConnectionZDRAcknowledgement(ctx context.Context, in NewConnectionZDRAcknowledgement) (ConnectionZDRAcknowledgement, bool, error) {
	if err := in.Validate(); err != nil {
		return ConnectionZDRAcknowledgement{}, false, err
	}
	var saved ConnectionZDRAcknowledgement
	var created bool
	err := r.inTransaction(ctx, func(q zdrQuerier) error {
		// The record is locked FOR SHARE so a concurrent digest change (which locks it FOR
		// UPDATE) waits for this insert and then revokes it, instead of racing past it.
		var organizationID, userID string
		var deletedAt *time.Time
		var capability, basis string
		var record ConnectionZDR
		err := q.QueryRow(ctx, `
			SELECT z.connection_id, z.provider_key, z.auth_mode, z.capability, z.basis, z.basis_ref,
			       z.catalog_version, z.digest, z.checked_at, z.created_at, z.updated_at,
			       c.organization_id, c.user_id, c.deleted_at
			FROM integration_connection_zdr AS z
			JOIN integration_connections AS c ON c.id = z.connection_id
			WHERE z.connection_id = $1
			FOR SHARE OF z`, in.ConnectionID).Scan(
			&record.ConnectionID, &record.ProviderKey, &record.AuthMode, &capability, &basis, &record.BasisRef,
			&record.CatalogVersion, &record.Digest, &record.CheckedAt, &record.CreatedAt, &record.UpdatedAt,
			&organizationID, &userID, &deletedAt)
		if errors.Is(err, pgx.ErrNoRows) {
			return ErrNotFound
		}
		if err != nil {
			return fmt.Errorf("lock connection zdr record for acknowledgement: %w", err)
		}
		record.Capability = zdr.Capability(capability)
		record.Basis = zdr.Basis(basis)
		if deletedAt != nil || organizationID != in.OrganizationID || userID != in.UserID {
			return ErrNotFound
		}
		if !record.Capability.RequiresAcknowledgement() {
			return invalidZDR("a %s connection needs no acknowledgement", record.Capability)
		}
		if record.Digest != in.Digest {
			return ErrConflict
		}
		id := strings.TrimSpace(in.ID)
		if id == "" {
			id = "zdrack_" + uuid.NewString()
		}
		at := in.AcknowledgedAt
		if at.IsZero() {
			at = time.Now().UTC()
		}
		inserted, err := scanZDRAcknowledgement(q.QueryRow(ctx, `
			INSERT INTO integration_connection_zdr_acknowledgements (`+zdrAcknowledgementColumnList+`)
			VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,NULL,'')
			ON CONFLICT (connection_id, digest) WHERE revoked_at IS NULL DO NOTHING
			RETURNING `+zdrAcknowledgementColumnList,
			id, organizationID, in.ConnectionID, userID, record.ProviderKey, string(record.Capability), string(record.Basis),
			record.CatalogVersion, record.Digest, in.TextVersion, in.Locale, in.TextSHA256, in.TextSnapshot,
			in.OrgZDRAtAck, in.AcknowledgedVia, in.RequestID, at.UTC()))
		switch {
		case err == nil:
			saved, created = inserted, true
			return nil
		case pgErrorCode(err) == pgUniqueViolation:
			return ErrConflict
		case !errors.Is(err, pgx.ErrNoRows):
			return fmt.Errorf("insert zdr acknowledgement: %w", err)
		}
		// An active acknowledgement for this connection and digest already exists: this is a
		// replay, and the stored row is the answer.
		existing, err := scanZDRAcknowledgement(q.QueryRow(ctx, `
			SELECT `+zdrAcknowledgementColumnList+`
			FROM integration_connection_zdr_acknowledgements
			WHERE connection_id = $1 AND digest = $2 AND revoked_at IS NULL`,
			in.ConnectionID, record.Digest))
		if errors.Is(err, pgx.ErrNoRows) {
			// Revoked between the conflict and this read: not a replay, so say so and let the
			// caller retry against the new state.
			return ErrConflict
		}
		if err != nil {
			return fmt.Errorf("read existing zdr acknowledgement: %w", err)
		}
		saved, created = existing, false
		return nil
	})
	if err != nil {
		return ConnectionZDRAcknowledgement{}, false, err
	}
	return saved, created, nil
}

func (r *PostgresRepository) ListConnectionZDRAcknowledgements(ctx context.Context, connectionID string, activeOnly bool) ([]ConnectionZDRAcknowledgement, error) {
	rows, err := r.pool.Query(ctx, `
		SELECT `+zdrAcknowledgementColumnList+`
		FROM integration_connection_zdr_acknowledgements
		WHERE connection_id = $1 AND ($2::boolean = FALSE OR revoked_at IS NULL)
		ORDER BY acknowledged_at DESC, id ASC`, connectionID, activeOnly)
	if err != nil {
		return nil, fmt.Errorf("list zdr acknowledgements: %w", err)
	}
	return collectZDRAcknowledgements(rows)
}

func (r *PostgresRepository) RevokeConnectionZDRAcknowledgements(ctx context.Context, connectionID, reason string, at time.Time) ([]ConnectionZDRAcknowledgement, error) {
	if !ValidZDRRevokeReason(reason) {
		return nil, invalidZDR("revoke reason %q is not one of %s", reason, strings.Join(zdrRevokeReasons, ", "))
	}
	if at.IsZero() {
		at = time.Now().UTC()
	}
	rows, err := r.pool.Query(ctx, `
		UPDATE integration_connection_zdr_acknowledgements
		SET revoked_at = $2, revoke_reason = $3
		WHERE connection_id = $1 AND revoked_at IS NULL
		RETURNING `+zdrAcknowledgementColumnList, connectionID, at.UTC(), reason)
	if err != nil {
		return nil, fmt.Errorf("revoke zdr acknowledgements: %w", err)
	}
	return collectZDRAcknowledgements(rows)
}
