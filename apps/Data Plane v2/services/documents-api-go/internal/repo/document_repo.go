package repo

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/dataplane/services/documents-api-go/internal/model"
)

var ErrIdempotencyOwnershipConflict = errors.New("idempotency key belongs to another principal")

type DocumentRepo struct {
	pool *pgxpool.Pool
}

func NewDocumentRepo(pool *pgxpool.Pool) *DocumentRepo {
	return &DocumentRepo{pool: pool}
}

// documentColumns is the canonical projection — kept in one place so every read
// path and the scanners stay in lockstep (owner_id/visibility were added by the
// Per-User Data Ownership phase).
const documentColumns = `document_id, org_id, source, type, title, content, status, metadata,
	       error_message, zdr_classification, zdr_reason, extraction_trace,
	       created_by, deleted_by, created_at, updated_at, deleted_at, owner_id, visibility`

// Get returns a single document, enforcing ownership when a viewer is supplied.
// The viewer filter is a single static predicate so the tenant-isolation lint
// still sees org_id in the same literal: when viewerID is empty (no identity —
// legacy/back-compat) the `$3 = ”` branch short-circuits to the org-scoped
// behaviour; when present, only the owner, ORG-visible docs, or docs explicitly
// granted to the viewer (grantedIDs from user-core resource_grants) are
// returned. NOTE: 'shared' docs are NOT org-readable — they reach recipients
// ONLY via an explicit grant (or the owner), so visibility='shared' alone
// grants no one access. A filtered-out doc returns pgx.ErrNoRows → 404.
func (r *DocumentRepo) Get(ctx context.Context, orgID, documentID, viewerID string, grantedIDs []string) (*model.Document, error) {
	row := r.pool.QueryRow(ctx, `
		SELECT `+documentColumns+`
		FROM documents
		WHERE document_id = $1 AND org_id = $2 AND deleted_at IS NULL
		  AND ($3 = '' OR owner_id = $3 OR visibility = 'org' OR document_id = ANY($4))
	`, documentID, orgID, viewerID, normalizeGranted(grantedIDs))
	return scanDocument(row)
}

func (r *DocumentRepo) List(ctx context.Context, input model.ListDocumentsInput) (*model.ListDocumentsResult, error) {
	limit := input.Limit
	if limit <= 0 || limit > 100 {
		limit = 50
	}
	granted := normalizeGranted(input.GrantedIDs)

	// Static queries (one per filter shape) keep the static tenant-isolation
	// check effective and make the SQL audit-friendly. The ownership predicate
	// is the same parameterized clause in every shape; an empty ViewerID makes
	// it a no-op (org-scoped legacy behaviour).
	var total int
	if input.Type == "" {
		err := r.pool.QueryRow(ctx,
			`SELECT COUNT(*) FROM documents
			 WHERE org_id = $1 AND deleted_at IS NULL
			   AND ($2 = '' OR owner_id = $2 OR visibility = 'org' OR document_id = ANY($3))`,
			input.OrgID, input.ViewerID, granted,
		).Scan(&total)
		if err != nil {
			return nil, fmt.Errorf("count documents: %w", err)
		}
	} else {
		err := r.pool.QueryRow(ctx,
			`SELECT COUNT(*) FROM documents
			 WHERE org_id = $1 AND deleted_at IS NULL AND type = $2
			   AND ($3 = '' OR owner_id = $3 OR visibility = 'org' OR document_id = ANY($4))`,
			input.OrgID, input.Type, input.ViewerID, granted,
		).Scan(&total)
		if err != nil {
			return nil, fmt.Errorf("count documents: %w", err)
		}
	}

	var rows pgx.Rows
	var err error
	if input.Type == "" {
		rows, err = r.pool.Query(ctx, `
			SELECT `+documentColumns+`
			FROM documents
			WHERE org_id = $1 AND deleted_at IS NULL
			  AND ($2 = '' OR owner_id = $2 OR visibility = 'org' OR document_id = ANY($3))
			ORDER BY created_at DESC
			LIMIT $4 OFFSET $5
		`, input.OrgID, input.ViewerID, granted, limit, input.Offset)
	} else {
		rows, err = r.pool.Query(ctx, `
			SELECT `+documentColumns+`
			FROM documents
			WHERE org_id = $1 AND deleted_at IS NULL AND type = $2
			  AND ($3 = '' OR owner_id = $3 OR visibility = 'org' OR document_id = ANY($4))
			ORDER BY created_at DESC
			LIMIT $5 OFFSET $6
		`, input.OrgID, input.Type, input.ViewerID, granted, limit, input.Offset)
	}
	if err != nil {
		return nil, fmt.Errorf("list documents: %w", err)
	}
	defer rows.Close()

	var docs []model.Document
	for rows.Next() {
		d, err := scanDocumentFromRows(rows)
		if err != nil {
			return nil, err
		}
		docs = append(docs, *d)
	}

	return &model.ListDocumentsResult{Documents: docs, Total: total}, nil
}

// SourceCount is one (source, document_count) row returned by the
// per-org distinct-sources facet. Used by the velion dashboard's
// "Sources" stat (see ui-ux-velion-gap.md §U1-2).
type SourceCount struct {
	Source        string `json:"source"`
	DocumentCount int    `json:"document_count"`
}

// SourcesFacet returns the distinct list of sources for an org with a
// document count per source. Implements U1-2 (ui-ux-velion-gap.md §10):
// Quarry-v2 writes every scrape into this table with its `source` URL,
// so the dashboard's sources count = COUNT(DISTINCT source) here.
//
// The per-viewer visibility predicate is IDENTICAL to Get/List so the facet can
// never leak the existence (or count) of documents the viewer cannot read: only
// the viewer's own docs, ORG-visible docs, or docs explicitly granted to them
// are counted. An empty viewerID keeps the legacy org-scoped behaviour (no
// identity → back-compat), matching the other read paths.
func (r *DocumentRepo) SourcesFacet(ctx context.Context, orgID, viewerID string, grantedIDs []string) ([]SourceCount, int, error) {
	rows, err := r.pool.Query(ctx, `
		SELECT source, COUNT(*) AS document_count
		FROM documents
		WHERE org_id = $1 AND deleted_at IS NULL AND COALESCE(source, '') <> ''
		  AND ($2 = '' OR owner_id = $2 OR visibility = 'org' OR document_id = ANY($3))
		GROUP BY source
		ORDER BY document_count DESC, source ASC
	`, orgID, viewerID, normalizeGranted(grantedIDs))
	if err != nil {
		return nil, 0, fmt.Errorf("sources facet: %w", err)
	}
	defer rows.Close()

	out := make([]SourceCount, 0)
	for rows.Next() {
		var sc SourceCount
		if err := rows.Scan(&sc.Source, &sc.DocumentCount); err != nil {
			return nil, 0, fmt.Errorf("scan source facet: %w", err)
		}
		out = append(out, sc)
	}
	return out, len(out), nil
}

// CreateResult signals whether the create produced a new row or returned an
// existing one matched by idempotency_key.
type CreateResult struct {
	Document *model.Document
	// Reused is true when an idempotent re-ingest carried identical content —
	// a true no-op; no downstream event should fire.
	Reused bool
	// Updated is true when an idempotent re-ingest carried NEW content: the
	// stored row was refreshed in place and a documents.updated event must
	// fire so chunking/embedding re-run over the prior vectors.
	Updated bool
}

type OutboxEventFactory func(document *model.Document, updated bool) (eventType string, payload []byte, err error)

// CreateWithOutbox is the production mutation path. The document insert or
// content refresh, cache-version bump, and lifecycle event intent commit in one
// PostgreSQL transaction. True idempotent reuse emits nothing.
func (r *DocumentRepo) CreateWithOutbox(
	ctx context.Context,
	input model.CreateDocumentInput,
	eventFactory OutboxEventFactory,
) (*CreateResult, error) {
	if eventFactory == nil {
		return nil, fmt.Errorf("document lifecycle outbox factory required")
	}
	meta := input.Metadata
	if meta == nil {
		meta = json.RawMessage(`{}`)
	}
	zdr := input.ZDRClassification
	if zdr == "" {
		zdr = "internal"
	}
	trace := input.ExtractionTrace
	if trace == nil {
		trace = json.RawMessage(`{}`)
	}
	ownerID := input.OwnerID
	if ownerID == "" {
		ownerID = input.CreatedBy
	}
	if ownerID == "" {
		ownerID = ownerSystemAccount
	}
	visibility := normalizeVisibility(input.Visibility)

	for attempt := 0; attempt < 2; attempt++ {
		tx, err := r.pool.Begin(ctx)
		if err != nil {
			return nil, fmt.Errorf("begin document create: %w", err)
		}
		if input.IdempotencyKey != "" {
			row := tx.QueryRow(ctx, `SELECT `+documentColumns+`
				FROM documents WHERE org_id=$1 AND idempotency_key=$2 AND deleted_at IS NULL
				FOR UPDATE`, input.OrgID, input.IdempotencyKey)
			existing, lookupErr := scanDocument(row)
			if lookupErr != nil && !errors.Is(lookupErr, pgx.ErrNoRows) {
				tx.Rollback(ctx)
				return nil, fmt.Errorf("lookup document idempotency key: %w", lookupErr)
			}
			if lookupErr == nil && existing != nil {
				if !idempotencyOwnerMatches(existing, ownerID) {
					tx.Rollback(ctx)
					return nil, ErrIdempotencyOwnershipConflict
				}
				if documentContentUnchanged(existing, input) {
					if err := tx.Commit(ctx); err != nil {
						return nil, fmt.Errorf("commit document reuse: %w", err)
					}
					return &CreateResult{Document: existing, Reused: true}, nil
				}
				updatedRow := tx.QueryRow(ctx, `
					UPDATE documents SET source=$3,type=$4,title=$5,content=$6,metadata=$7,
					zdr_classification=$8,extraction_trace=$9,status='pending',error_message=NULL,
					deleted_at=NULL,updated_at=NOW()
					WHERE org_id=$1 AND document_id=$2 AND owner_id=$10
					RETURNING `+documentColumns,
					input.OrgID, existing.DocumentID, input.Source, input.Type, input.Title,
					input.Content, meta, zdr, trace, ownerID)
				updated, updateErr := scanDocument(updatedRow)
				if updateErr != nil {
					tx.Rollback(ctx)
					return nil, fmt.Errorf("update document content: %w", updateErr)
				}
				if err := enqueueLifecycleEventTx(ctx, tx, input.OrgID, updated, true, eventFactory); err != nil {
					return nil, err
				}
				if err := bumpOrgVersionTx(ctx, tx, input.OrgID); err != nil {
					return nil, err
				}
				if err := tx.Commit(ctx); err != nil {
					return nil, fmt.Errorf("commit document update: %w", err)
				}
				return &CreateResult{Document: updated, Updated: true}, nil
			}
		}

		row := tx.QueryRow(ctx, `
			INSERT INTO documents (org_id,source,type,title,content,metadata,zdr_classification,
				extraction_trace,created_by,owner_id,visibility,idempotency_key,status)
			VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,'pending')
			RETURNING `+documentColumns,
			input.OrgID, input.Source, input.Type, input.Title, input.Content, meta, zdr, trace,
			nilIfEmpty(input.CreatedBy), ownerID, visibility, nilIfEmpty(input.IdempotencyKey))
		doc, insertErr := scanDocument(row)
		if insertErr != nil {
			tx.Rollback(ctx)
			if input.IdempotencyKey != "" && isUniqueViolation(insertErr) && attempt == 0 {
				continue
			}
			return nil, insertErr
		}
		if err := enqueueLifecycleEventTx(ctx, tx, input.OrgID, doc, false, eventFactory); err != nil {
			return nil, err
		}
		if err := bumpOrgVersionTx(ctx, tx, input.OrgID); err != nil {
			return nil, err
		}
		if err := tx.Commit(ctx); err != nil {
			return nil, fmt.Errorf("commit document create: %w", err)
		}
		return &CreateResult{Document: doc}, nil
	}
	return nil, fmt.Errorf("document idempotency race did not converge")
}

func enqueueLifecycleEventTx(
	ctx context.Context,
	tx pgx.Tx,
	orgID string,
	document *model.Document,
	updated bool,
	factory OutboxEventFactory,
) error {
	eventType, payload, err := factory(document, updated)
	if err != nil || strings.TrimSpace(eventType) == "" || !json.Valid(payload) {
		tx.Rollback(ctx)
		return fmt.Errorf("build document lifecycle event: %w", err)
	}
	if _, err := tx.Exec(ctx, `INSERT INTO documents_outbox (org_id,event_type,payload)
		VALUES ($1,$2,$3::jsonb)`, orgID, eventType, string(payload)); err != nil {
		tx.Rollback(ctx)
		return fmt.Errorf("enqueue document lifecycle event: %w", err)
	}
	return nil
}

func bumpOrgVersionTx(ctx context.Context, tx pgx.Tx, orgID string) error {
	if _, err := tx.Exec(ctx, `INSERT INTO org_versions (org_id,version,bumped_at)
		VALUES ($1,2,NOW()) ON CONFLICT (org_id) DO UPDATE
		SET version=org_versions.version+1,bumped_at=NOW()`, orgID); err != nil {
		tx.Rollback(ctx)
		return fmt.Errorf("bump org version: %w", err)
	}
	return nil
}

func (r *DocumentRepo) Create(ctx context.Context, input model.CreateDocumentInput) (*CreateResult, error) {
	meta := input.Metadata
	if meta == nil {
		meta = json.RawMessage(`{}`)
	}
	zdr := input.ZDRClassification
	if zdr == "" {
		zdr = "internal"
	}
	trace := input.ExtractionTrace
	if trace == nil {
		trace = json.RawMessage(`{}`)
	}

	// Stamp ownership: explicit owner wins, else the creator, else the system
	// account (matches the column default + grandfather sentinel). Visibility
	// defaults to 'org' — Private is an explicit opt-in.
	ownerID := input.OwnerID
	if ownerID == "" {
		ownerID = input.CreatedBy
	}
	if ownerID == "" {
		ownerID = ownerSystemAccount
	}
	visibility := normalizeVisibility(input.Visibility)

	// Idempotency: if key supplied and a non-deleted row exists for (org, key),
	// return that row instead of inserting. We do the lookup BEFORE insert so the
	// hot path stays cheap when no key is supplied.
	if input.IdempotencyKey != "" {
		existing, err := r.findByIdempotencyKey(ctx, input.OrgID, input.IdempotencyKey)
		if err == nil && existing != nil {
			if !idempotencyOwnerMatches(existing, ownerID) {
				return nil, ErrIdempotencyOwnershipConflict
			}
			if documentContentUnchanged(existing, input) {
				return &CreateResult{Document: existing, Reused: true}, nil
			}
			// Same logical document, new content: refresh in place and signal
			// an update so chunking/embedding re-run and overwrite the prior
			// vectors instead of leaving stale content (or a duplicate row).
			updated, uerr := r.updateContent(ctx, input.OrgID, existing.DocumentID, ownerID, input)
			if uerr != nil {
				return nil, fmt.Errorf("update document content: %w", uerr)
			}
			r.bumpOrgVersion(ctx, input.OrgID)
			return &CreateResult{Document: updated, Updated: true}, nil
		}
	}

	row := r.pool.QueryRow(ctx, `
		INSERT INTO documents (org_id, source, type, title, content, metadata, zdr_classification, extraction_trace, created_by, owner_id, visibility, idempotency_key, status)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, 'pending')
		RETURNING `+documentColumns+`
	`, input.OrgID, input.Source, input.Type, input.Title, input.Content, meta, zdr, trace,
		nilIfEmpty(input.CreatedBy), ownerID, visibility, nilIfEmpty(input.IdempotencyKey))

	doc, err := scanDocument(row)
	if err != nil {
		// Race: another concurrent insert won the unique-index battle; recover by
		// returning the row that committed first.
		if input.IdempotencyKey != "" && isUniqueViolation(err) {
			existing, lookupErr := r.findByIdempotencyKey(ctx, input.OrgID, input.IdempotencyKey)
			if lookupErr == nil && existing != nil {
				if !idempotencyOwnerMatches(existing, ownerID) {
					return nil, ErrIdempotencyOwnershipConflict
				}
				return &CreateResult{Document: existing, Reused: true}, nil
			}
		}
		return nil, err
	}
	// §16.2.2 — new document means any cached retrieval for this org is
	// potentially stale. Bump the version so the cache key becomes
	// unreachable instantly.
	r.bumpOrgVersion(ctx, input.OrgID)
	return &CreateResult{Document: doc, Reused: false}, nil
}

func idempotencyOwnerMatches(existing *model.Document, requestedOwner string) bool {
	return existing != nil && existing.OwnerID != "" && existing.OwnerID == requestedOwner
}

// documentContentUnchanged reports whether an idempotent re-ingest carries the
// same indexable text as the stored document. Only content + title drive
// chunking and embeddings, so a match means the re-ingest is a true no-op and
// no update event should fire. (Metadata-only changes are intentionally treated
// as no-ops here; they don't require re-embedding.)
func documentContentUnchanged(existing *model.Document, input model.CreateDocumentInput) bool {
	return existing.Content == input.Content && existing.Title == input.Title
}

// updateContent refreshes an existing document in place with re-ingested
// content and resets it to 'pending' so the chunking/embedding pipeline
// reprocesses it. deleted_at is cleared so a re-ingest also resurrects a
// previously soft-deleted document. Ownership + visibility are intentionally
// left untouched — a re-ingest must not silently re-open a privately-scoped doc.
func (r *DocumentRepo) updateContent(ctx context.Context, orgID, documentID, ownerID string, input model.CreateDocumentInput) (*model.Document, error) {
	meta := input.Metadata
	if meta == nil {
		meta = json.RawMessage(`{}`)
	}
	zdr := input.ZDRClassification
	if zdr == "" {
		zdr = "internal"
	}
	trace := input.ExtractionTrace
	if trace == nil {
		trace = json.RawMessage(`{}`)
	}

	row := r.pool.QueryRow(ctx, `
		UPDATE documents
		   SET source = $3, type = $4, title = $5, content = $6, metadata = $7,
		       zdr_classification = $8, extraction_trace = $9, status = 'pending',
		       error_message = NULL, deleted_at = NULL, updated_at = NOW()
		 WHERE org_id = $1 AND document_id = $2 AND owner_id = $10
		RETURNING `+documentColumns+`
	`, orgID, documentID, input.Source, input.Type, input.Title, input.Content, meta, zdr, trace, ownerID)

	return scanDocument(row)
}

func (r *DocumentRepo) findByIdempotencyKey(ctx context.Context, orgID, key string) (*model.Document, error) {
	row := r.pool.QueryRow(ctx, `
		SELECT `+documentColumns+`
		FROM documents
		WHERE org_id = $1 AND idempotency_key = $2 AND deleted_at IS NULL
	`, orgID, key)
	return scanDocument(row)
}

func isUniqueViolation(err error) bool {
	return err != nil && (containsCode(err, "23505") || containsAny(err.Error(), "duplicate key", "unique constraint"))
}

func containsCode(err error, code string) bool {
	type pgErr interface {
		SQLState() string
	}
	if pe, ok := err.(pgErr); ok {
		return pe.SQLState() == code
	}
	return false
}

func containsAny(s string, subs ...string) bool {
	for _, sub := range subs {
		for i := 0; i+len(sub) <= len(s); i++ {
			if s[i:i+len(sub)] == sub {
				return true
			}
		}
	}
	return false
}

func (r *DocumentRepo) SoftDelete(ctx context.Context, orgID, documentID, deletedBy string) error {
	tag, err := r.pool.Exec(ctx, `
		UPDATE documents SET deleted_at = NOW(), deleted_by = $3
		WHERE document_id = $1 AND org_id = $2 AND deleted_at IS NULL
	`, documentID, orgID, nilIfEmpty(deletedBy))
	if err != nil {
		return fmt.Errorf("soft delete document: %w", err)
	}
	if tag.RowsAffected() == 0 {
		return fmt.Errorf("document not found")
	}
	// §16.2.2 — bump org_version so any cached retrieval results for this
	// org become unreachable immediately. Best-effort: a failure here logs
	// but does not propagate; the cache will still age out via TTL.
	r.bumpOrgVersion(ctx, orgID)
	return nil
}

// SoftDeleteWithOutbox atomically commits the tenant-scoped soft delete,
// retrieval cache-version bump, and signed-event intent. A crash can occur at
// any later point without losing the deletion notification.
func (r *DocumentRepo) SoftDeleteWithOutbox(
	ctx context.Context,
	orgID, documentID, deletedBy, eventType string,
	payload []byte,
) error {
	if strings.TrimSpace(orgID) == "" || strings.TrimSpace(documentID) == "" ||
		strings.TrimSpace(eventType) == "" || !json.Valid(payload) {
		return fmt.Errorf("soft delete outbox requires scoped identity and valid payload")
	}
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("begin soft delete: %w", err)
	}
	defer tx.Rollback(ctx)
	tag, err := tx.Exec(ctx, `
		UPDATE documents SET deleted_at = NOW(), deleted_by = $3
		WHERE document_id = $1 AND org_id = $2 AND deleted_at IS NULL
	`, documentID, orgID, nilIfEmpty(deletedBy))
	if err != nil {
		return fmt.Errorf("soft delete document: %w", err)
	}
	if tag.RowsAffected() == 0 {
		return fmt.Errorf("document not found")
	}
	if _, err := tx.Exec(ctx, `
		INSERT INTO documents_outbox (org_id, event_type, payload)
		VALUES ($1, $2, $3::jsonb)
	`, orgID, eventType, string(payload)); err != nil {
		return fmt.Errorf("enqueue deletion outbox: %w", err)
	}
	if _, err := tx.Exec(ctx, `
		INSERT INTO org_versions (org_id, version, bumped_at)
		VALUES ($1, 2, NOW())
		ON CONFLICT (org_id) DO UPDATE
		SET version = org_versions.version + 1, bumped_at = NOW()
	`, orgID); err != nil {
		return fmt.Errorf("bump org version: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("commit soft delete and outbox: %w", err)
	}
	return nil
}

// EnqueueOutbox writes a row to `documents_outbox` for the publisher
// loop to drain. §16.2.6 — the caller can rely on at-least-once
// delivery without needing to know whether NATS is up. `eventType` is
// used directly as the NATS subject by the publisher loop.
func (r *DocumentRepo) EnqueueOutbox(ctx context.Context, orgID, eventType string, payload []byte) error {
	_, err := r.pool.Exec(ctx, `
		INSERT INTO documents_outbox (org_id, event_type, payload)
		VALUES ($1, $2, $3::jsonb)
	`, orgID, eventType, payload)
	if err != nil {
		return fmt.Errorf("enqueue outbox: %w", err)
	}
	return nil
}

// TransferOwnership reassigns every document owned by fromOwner in an org to
// toOwner. Used by the GDPR erasure subscriber: when a user is erased their
// owned documents move to the org system account (or an admin), so no orphaned
// owner_id remains and no human silently inherits the erased user's private
// docs (org-visible docs keep their visibility; private docs become system-owned
// and thus visible to no human until an admin re-shares them). Idempotent:
// re-running after the first transfer matches zero rows. Returns the count moved.
func (r *DocumentRepo) TransferOwnership(ctx context.Context, orgID, fromOwner, toOwner string) (int64, error) {
	if fromOwner == "" || toOwner == "" {
		return 0, fmt.Errorf("transfer ownership requires non-empty from/to owner")
	}
	tag, err := r.pool.Exec(ctx, `
		UPDATE documents
		   SET owner_id = $3, updated_at = NOW()
		 WHERE org_id = $1 AND owner_id = $2
	`, orgID, fromOwner, toOwner)
	if err != nil {
		return 0, fmt.Errorf("transfer document ownership: %w", err)
	}
	n := tag.RowsAffected()
	if n > 0 {
		r.bumpOrgVersion(ctx, orgID)
	}
	return n, nil
}

// bumpOrgVersion increments the per-org cache-busting counter. Called
// from every mutating path. The Postgres UPSERT pattern handles both
// "first mutation for this org" and "Nth mutation" without branching.
func (r *DocumentRepo) bumpOrgVersion(ctx context.Context, orgID string) {
	_, err := r.pool.Exec(ctx, `
		INSERT INTO org_versions (org_id, version, bumped_at)
			VALUES ($1, 2, NOW())
		ON CONFLICT (org_id) DO UPDATE
			SET version = org_versions.version + 1,
			    bumped_at = NOW()
	`, orgID)
	if err != nil {
		// Don't fail the request — the cache still ages out via TTL.
		log.Warn().Err(err).Str("org_id", orgID).Msg("org_version bump failed")
	}
}

func scanDocument(row pgx.Row) (*model.Document, error) {
	var d model.Document
	err := row.Scan(
		&d.DocumentID, &d.OrgID, &d.Source, &d.Type, &d.Title, &d.Content,
		&d.Status, &d.Metadata, &d.ErrorMessage, &d.ZDRClassification, &d.ZDRReason,
		&d.ExtractionTrace, &d.CreatedBy, &d.DeletedBy, &d.CreatedAt, &d.UpdatedAt, &d.DeletedAt,
		&d.OwnerID, &d.Visibility,
	)
	if err != nil {
		return nil, fmt.Errorf("scan document: %w", err)
	}
	return &d, nil
}

func scanDocumentFromRows(rows pgx.Rows) (*model.Document, error) {
	var d model.Document
	err := rows.Scan(
		&d.DocumentID, &d.OrgID, &d.Source, &d.Type, &d.Title, &d.Content,
		&d.Status, &d.Metadata, &d.ErrorMessage, &d.ZDRClassification, &d.ZDRReason,
		&d.ExtractionTrace, &d.CreatedBy, &d.DeletedBy, &d.CreatedAt, &d.UpdatedAt, &d.DeletedAt,
		&d.OwnerID, &d.Visibility,
	)
	if err != nil {
		return nil, fmt.Errorf("scan document row: %w", err)
	}
	return &d, nil
}

func nilIfEmpty(s string) *string {
	if s == "" {
		return nil
	}
	return &s
}

// ownerSystemAccount is the sentinel owner for grandfathered/non-API documents.
// Matches the documents.owner_id column DEFAULT.
const ownerSystemAccount = "org-system-account"

// normalizeVisibility clamps an incoming visibility to the allowed domain,
// defaulting to 'org' (the non-breaking, org-shared default).
func normalizeVisibility(v string) string {
	switch v {
	case "private", "org", "shared":
		return v
	default:
		// Fail-safe: an unset/invalid value defaults to PRIVATE, never org-wide.
		// The handler (applyVisibilityPolicy) sets an explicit value first — for
		// end users that's 'private', for system ingest 'org' — so this branch is
		// only a backstop against an unexpected/invalid string.
		return "private"
	}
}

// normalizeGranted guarantees a non-nil slice so `= ANY($n)` binds to an empty
// text[] (matching nothing) rather than NULL.
func normalizeGranted(ids []string) []string {
	if ids == nil {
		return []string{}
	}
	return ids
}
