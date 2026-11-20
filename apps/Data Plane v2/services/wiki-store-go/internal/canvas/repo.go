// Package canvas stores the room canvas (WS19): one revisioned Markdown document per Space, in tables of its own
// inside wiki-store.
//
// Room text never goes to wiki_pages or wiki_page_versions. Every wiki publication emits
// dataplane.wiki.version.published with full content into the org-wide embedding stream, so a canvas kept there would
// leak room text to the whole org. This package writes no outbox row and publishes no event.
//
// The package enforces the head compare-and-swap, idempotency, limits, and the org and Space scope of every row. It does
// not decide authority: the HTTP layer verifies the Control-signed Space decision and sets Author and Provenance from it
// before it calls a write. A member's history floor windows the version history, never the current head (see Floor in window.go).
package canvas

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/triodelab/dataplane/shared/go/orgscope"
)

// Author kinds. A version's author kind comes from the verified decision envelope, never from a request body.
const (
	AuthorUser  = "user"
	AuthorAgent = "agent"
)

// Operations. They are part of the idempotency binding.
const (
	opSave    = "save"
	opRestore = "restore"
)

// Change kinds stored on a version.
const (
	changeCreate  = "create"
	changeEdit    = "edit"
	changeRestore = "restore"
)

// Error codes. They are the wire codes the HTTP layer returns in {error:{code,message,details}}. The layer picks the
// HTTP status for each.
const (
	CodeCanvasNotFound         = "canvas_not_found"
	CodeVersionNotFound        = "canvas_version_not_found"
	CodeHeadMoved              = "canvas_head_moved"
	CodeIdempotencyKeyConflict = "idempotency_key_conflict"
	CodeTooLarge               = "canvas_too_large"
	CodeInvalidRequest         = "invalid_canvas_request"
	CodeInvalidCursor          = "invalid_cursor"
	CodeRateLimited            = "canvas_rate_limited"
	CodeRunWriteLimit          = "canvas_run_write_limit"
	// CodeHistoryFloor is the refusal of a version written before the reader's history window (window.go). It is the wire code
	// Control's decision refusal uses, so one code names the floor on both planes.
	CodeHistoryFloor = "space_history_floor_unsupported"
)

// Error is a typed refusal from this package. Head is set for CodeHeadMoved (nil when the Space has no canvas yet) and
// MaxBytes for CodeTooLarge.
type Error struct {
	Code     string
	Message  string
	Head     *HeadRef
	MaxBytes int
}

func (e *Error) Error() string {
	return e.Code + ": " + e.Message
}

// Author is the verified principal that stamps a version. For an agent, ID is "run:" + RunID and OnBehalfOf is the human
// who invoked the run. For a user, ID is the decision subject and the run fields stay empty.
type Author struct {
	Kind       string
	ID         string
	OnBehalfOf string
	RunID      string
}

// Provenance is the Control decision a write was made under. It is provenance, not authority: the HTTP layer verifies
// the signed decision before it calls a write, and the repository refuses a write whose provenance is incomplete.
type Provenance struct {
	DecisionRef       string
	AuthorityRevision int64
	AudienceRevision  int64
}

// Canvas is the metadata of a Space's active canvas.
type Canvas struct {
	CanvasID      string
	SpaceRef      string
	State         string
	HeadVersionID *string
	HeadVersionNo int
	CreatedAt     time.Time
	UpdatedAt     time.Time
}

// HeadRef names a version without its content.
type HeadRef struct {
	VersionID  string
	VersionNo  int
	AuthorKind string
	CreatedAt  time.Time
}

// Version is one stored version. Content is filled only by GetVersion and Head.
type Version struct {
	VersionID             string
	CanvasID              string
	VersionNo             int
	ParentVersionID       *string
	AuthorKind            string
	AuthorID              string
	OnBehalfOf            *string
	RunID                 *string
	ChangeKind            string
	RestoredFromVersionID *string
	Summary               *string
	ContentBytes          int
	ContentSHA256         string
	CreatedAt             time.Time
	Content               string
	// Current is set by ListVersions on the canvas's current head.
	Current bool
}

// HeadView is a canvas and its head. When the caller already holds the head, Unchanged is set, Head is nil and no content
// is read.
type HeadView struct {
	Canvas    Canvas
	Head      *Version
	Unchanged bool
}

// VersionPage is one page of history, newest first and without content. NextBefore is the cursor for the next page, nil
// when there is none. Under a history floor, HiddenBefore reports that the canvas holds versions the window withholds, so the
// list is never silently short (D21m).
type VersionPage struct {
	Versions     []Version
	NextBefore   *int
	HiddenBefore bool
}

// SaveInput is one save. CanvasID is the canvas the Control decision named: the Space's active canvas for a save, or ""
// for a create. The write refuses unless the canvas it finds under the lock is exactly that one. ExpectedVersionID is the
// head the editor opened from. Nil means the editor believes the Space has no canvas yet.
type SaveInput struct {
	OrgID             string
	SpaceRef          string
	CanvasID          string
	ExpectedVersionID *string
	Content           string
	Summary           *string
	IdempotencyKey    string
	Author            Author
	Provenance        Provenance
}

// RestoreInput copies the text of one version of the same canvas into a new head, stamped change_kind restore. CanvasID is
// the canvas the decision named and is required: a restore never lands in any other canvas.
type RestoreInput struct {
	OrgID                string
	SpaceRef             string
	CanvasID             string
	RestoreFromVersionID string
	ExpectedVersionID    *string
	IdempotencyKey       string
	Author               Author
	Provenance           Provenance
	// Floor is the restoring member's history window. A version written before it, other than the current head, is refused
	// with CodeHistoryFloor (D21m). The zero Floor windows nothing.
	Floor Floor
}

// WriteResult reports a save or restore. Inserted, Deduplicated and Unchanged are mutually exclusive. Version is set
// when Inserted or Deduplicated.
type WriteResult struct {
	CanvasID      string
	Version       Version
	Inserted      bool
	Deduplicated  bool
	Unchanged     bool
	HeadVersionID string
}

// PurgeCounts reports the rows a purge hard-deleted.
type PurgeCounts struct {
	CanvasesPurged int64
	VersionsPurged int64
}

// Repo is the canvas store. Reads and writes run in orgscope transactions, so the restricted dataplane_app role and the
// RLS policies apply. Purge runs on the unscoped pool, like HardPurgeByOrg, because the app role holds no DELETE on
// versions.
type Repo struct {
	pool *pgxpool.Pool
}

// NewRepo returns the canvas store over the service pool.
func NewRepo(pool *pgxpool.Pool) *Repo {
	return &Repo{pool: pool}
}

// versionCols are the metadata columns of a version, in the order scanVersion reads them.
const versionCols = `version_id, canvas_id, version_no, parent_version_id, author_kind, author_id,
	on_behalf_of, run_id, change_kind, restored_from_version_id, summary, content_bytes,
	content_sha256, created_at`

// canvasCols are the columns of a canvas, in the order scanCanvas reads them.
const canvasCols = `canvas_id, space_ref, state, head_version_id, head_version_no, created_at, updated_at`

// scanner is the Scan surface shared by pgx.Row and pgx.Rows.
type scanner interface {
	Scan(dest ...any) error
}

func scanVersion(row scanner, withContent bool) (Version, error) {
	var v Version
	dest := []any{
		&v.VersionID, &v.CanvasID, &v.VersionNo, &v.ParentVersionID, &v.AuthorKind, &v.AuthorID,
		&v.OnBehalfOf, &v.RunID, &v.ChangeKind, &v.RestoredFromVersionID, &v.Summary, &v.ContentBytes,
		&v.ContentSHA256, &v.CreatedAt,
	}
	if withContent {
		dest = append(dest, &v.Content)
	}
	if err := row.Scan(dest...); err != nil {
		return Version{}, err
	}
	return v, nil
}

func scanCanvas(row scanner) (Canvas, error) {
	var c Canvas
	if err := row.Scan(&c.CanvasID, &c.SpaceRef, &c.State, &c.HeadVersionID, &c.HeadVersionNo, &c.CreatedAt, &c.UpdatedAt); err != nil {
		return Canvas{}, err
	}
	return c, nil
}

// findActiveCanvas reads the Space's active canvas. With lock set, it also locks the canvas row until the transaction
// ends. That lock is the per-room serialisation: concurrent writers queue behind it and then compare against the head
// that the previous writer left.
func findActiveCanvas(ctx context.Context, tx pgx.Tx, orgID, spaceRef string, lock bool) (Canvas, bool, error) {
	query := `SELECT ` + canvasCols + ` FROM space_canvases
		WHERE org_id = $1 AND space_ref = $2 AND state = 'active'`
	if lock {
		query += ` FOR UPDATE`
	}
	c, err := scanCanvas(tx.QueryRow(ctx, query, orgID, spaceRef))
	if errors.Is(err, pgx.ErrNoRows) {
		return Canvas{}, false, nil
	}
	if err != nil {
		return Canvas{}, false, fmt.Errorf("read canvas: %w", err)
	}
	return c, true, nil
}

// loadVersion reads one version of one canvas. The caller maps pgx.ErrNoRows to a typed not-found.
func loadVersion(ctx context.Context, tx pgx.Tx, orgID, canvasID, versionID string, withContent bool) (Version, error) {
	query := `SELECT ` + versionCols
	if withContent {
		query += `, content`
	}
	query += ` FROM space_canvas_versions WHERE org_id = $1 AND canvas_id = $2 AND version_id = $3`
	return scanVersion(tx.QueryRow(ctx, query, orgID, canvasID, versionID), withContent)
}

// Save stores content as the canvas head when the caller holds the current head. It returns Deduplicated for a replay
// under the same idempotency key, Unchanged when the content equals the head, and a CodeHeadMoved error when the head has
// moved. A refused save stores nothing.
func (r *Repo) Save(ctx context.Context, in SaveInput) (WriteResult, error) {
	if err := validateScope(in.OrgID, in.SpaceRef); err != nil {
		return WriteResult{}, err
	}
	if err := validateAuthor(in.Author); err != nil {
		return WriteResult{}, err
	}
	if err := validateProvenance(in.Provenance); err != nil {
		return WriteResult{}, err
	}
	if err := ValidateIdempotencyKey(in.IdempotencyKey); err != nil {
		return WriteResult{}, err
	}
	if in.ExpectedVersionID != nil {
		if err := ValidateVersionID(*in.ExpectedVersionID); err != nil {
			return WriteResult{}, err
		}
	}
	if err := ValidateContent(in.Content); err != nil {
		return WriteResult{}, err
	}
	summary, err := NormalizeSummary(in.Summary)
	if err != nil {
		return WriteResult{}, err
	}
	return r.write(ctx, writeOp{
		op:       opSave,
		orgID:    in.OrgID,
		spaceRef: in.SpaceRef,
		canvasID: in.CanvasID,
		expected: in.ExpectedVersionID,
		content:  in.Content,
		summary:  summary,
		idemKey:  in.IdempotencyKey,
		author:   in.Author,
		prov:     in.Provenance,
	})
}

// Restore copies the text of one version of this canvas into a new head. It follows the same compare-and-swap and
// idempotency rules as Save. A restore never creates a canvas.
func (r *Repo) Restore(ctx context.Context, in RestoreInput) (WriteResult, error) {
	if err := validateScope(in.OrgID, in.SpaceRef); err != nil {
		return WriteResult{}, err
	}
	if in.CanvasID == "" {
		return WriteResult{}, invalidRequest("a restore must name the canvas the decision covers")
	}
	if err := validateAuthor(in.Author); err != nil {
		return WriteResult{}, err
	}
	if err := validateProvenance(in.Provenance); err != nil {
		return WriteResult{}, err
	}
	if err := ValidateIdempotencyKey(in.IdempotencyKey); err != nil {
		return WriteResult{}, err
	}
	if err := ValidateVersionID(in.RestoreFromVersionID); err != nil {
		return WriteResult{}, err
	}
	if in.ExpectedVersionID != nil {
		if err := ValidateVersionID(*in.ExpectedVersionID); err != nil {
			return WriteResult{}, err
		}
	}
	return r.write(ctx, writeOp{
		op:          opRestore,
		orgID:       in.OrgID,
		spaceRef:    in.SpaceRef,
		canvasID:    in.CanvasID,
		expected:    in.ExpectedVersionID,
		restoreFrom: in.RestoreFromVersionID,
		floor:       in.Floor,
		idemKey:     in.IdempotencyKey,
		author:      in.Author,
		prov:        in.Provenance,
	})
}

// Head returns the Space's canvas and its head. When knownVersionID already names the head, the result is Unchanged and
// carries no content. The 15-second poll uses that path. A history floor does not apply: every member who may read the canvas
// reads its current head (D21m).
func (r *Repo) Head(ctx context.Context, orgID, spaceRef string, knownVersionID *string) (HeadView, error) {
	if err := validateScope(orgID, spaceRef); err != nil {
		return HeadView{}, err
	}
	return orgscope.InOrgScope(ctx, r.pool, orgID, func(ctx context.Context, tx pgx.Tx) (HeadView, error) {
		cv, found, err := findActiveCanvas(ctx, tx, orgID, spaceRef, false)
		if err != nil {
			return HeadView{}, err
		}
		if !found {
			return HeadView{}, &Error{Code: CodeCanvasNotFound, Message: "the Space has no canvas"}
		}
		if cv.HeadVersionID == nil {
			return HeadView{}, fmt.Errorf("canvas %s has no head", cv.CanvasID)
		}
		if knownVersionID != nil && *knownVersionID == *cv.HeadVersionID {
			return HeadView{Canvas: cv, Unchanged: true}, nil
		}
		head, err := loadVersion(ctx, tx, orgID, cv.CanvasID, *cv.HeadVersionID, true)
		if err != nil {
			return HeadView{}, fmt.Errorf("load canvas head: %w", err)
		}
		return HeadView{Canvas: cv, Head: &head}, nil
	})
}

// ListVersions returns one page of history, newest first and without content. before is the version number the previous
// page ended at. Nil starts from the newest version. Under a floor it lists only the versions inside the window, plus the current
// head (marked Current) wherever it falls, and sets HiddenBefore when the canvas holds versions the window withholds (D21m).
func (r *Repo) ListVersions(ctx context.Context, orgID, spaceRef string, before *int, limit int, floor Floor) (VersionPage, error) {
	if err := validateScope(orgID, spaceRef); err != nil {
		return VersionPage{}, err
	}
	if before != nil && *before < 1 {
		return VersionPage{}, &Error{Code: CodeInvalidCursor, Message: "the cursor must be a version number"}
	}
	limit = ClampListLimit(limit)
	return orgscope.InOrgScope(ctx, r.pool, orgID, func(ctx context.Context, tx pgx.Tx) (VersionPage, error) {
		cv, found, err := findActiveCanvas(ctx, tx, orgID, spaceRef, false)
		if err != nil {
			return VersionPage{}, err
		}
		if !found {
			return VersionPage{}, &Error{Code: CodeCanvasNotFound, Message: "the Space has no canvas"}
		}
		// The window filter: the head, or a version inside every floor that is set. With no floor the instant is null and the
		// revision zero, so every version passes.
		rows, err := tx.Query(ctx, `SELECT `+versionCols+` FROM space_canvas_versions
			WHERE org_id = $1 AND canvas_id = $2
			  AND ($3::integer IS NULL OR version_no < $3::integer)
			  AND (version_id = $5 OR (($6::timestamptz IS NULL OR created_at >= $6::timestamptz) AND audience_revision >= $7))
			ORDER BY version_no DESC
			LIMIT $4`, orgID, cv.CanvasID, before, limit+1, headVersionID(cv), floor.At, floor.Revision)
		if err != nil {
			return VersionPage{}, fmt.Errorf("list canvas versions: %w", err)
		}
		defer rows.Close()
		versions := make([]Version, 0, limit)
		for rows.Next() {
			v, err := scanVersion(rows, false)
			if err != nil {
				return VersionPage{}, fmt.Errorf("read canvas version: %w", err)
			}
			v.Current = v.VersionID == headVersionID(cv)
			versions = append(versions, v)
		}
		if err := rows.Err(); err != nil {
			return VersionPage{}, fmt.Errorf("list canvas versions: %w", err)
		}
		page := VersionPage{Versions: versions}
		if floor.Set() {
			// rows is exhausted here, so the connection is free for the second statement.
			if err := tx.QueryRow(ctx, `SELECT EXISTS (SELECT 1 FROM space_canvas_versions
				WHERE org_id = $1 AND canvas_id = $2 AND version_id <> $3
				  AND (($4::timestamptz IS NOT NULL AND created_at < $4::timestamptz) OR audience_revision < $5))`,
				orgID, cv.CanvasID, headVersionID(cv), floor.At, floor.Revision).Scan(&page.HiddenBefore); err != nil {
				return VersionPage{}, fmt.Errorf("read canvas history window: %w", err)
			}
		}
		if len(versions) > limit {
			// The extra row only signals that another page exists; it is not returned.
			page.Versions = versions[:limit]
			next := versions[limit-1].VersionNo
			page.NextBefore = &next
		}
		return page, nil
	})
}

// GetVersion returns one version with its content. A version of another Space, another org or an unknown id is not
// found. Under a floor, a version written before the window answers CodeHistoryFloor unless it is the current head (D21m).
func (r *Repo) GetVersion(ctx context.Context, orgID, spaceRef, versionID string, floor Floor) (Version, error) {
	if err := validateScope(orgID, spaceRef); err != nil {
		return Version{}, err
	}
	if err := ValidateVersionID(versionID); err != nil {
		return Version{}, &Error{Code: CodeVersionNotFound, Message: "the version is not in this canvas"}
	}
	return orgscope.InOrgScope(ctx, r.pool, orgID, func(ctx context.Context, tx pgx.Tx) (Version, error) {
		v, err := scanVersion(tx.QueryRow(ctx, `SELECT `+versionCols+`, content FROM space_canvas_versions
			WHERE org_id = $1 AND version_id = $2
			  AND EXISTS (
				SELECT 1 FROM space_canvases c
				WHERE c.canvas_id = space_canvas_versions.canvas_id
				  AND c.org_id = space_canvas_versions.org_id
				  AND c.space_ref = $3
				  AND c.state = 'active')`,
			orgID, versionID, spaceRef), true)
		if errors.Is(err, pgx.ErrNoRows) {
			return Version{}, &Error{Code: CodeVersionNotFound, Message: "the version is not in this canvas"}
		}
		if err != nil {
			return Version{}, fmt.Errorf("read canvas version: %w", err)
		}
		if floor.Set() {
			inWindow, found, err := versionInWindow(ctx, tx, orgID, v.CanvasID, v.VersionID, floor)
			if err != nil {
				return Version{}, err
			}
			if !found {
				return Version{}, &Error{Code: CodeVersionNotFound, Message: "the version is not in this canvas"}
			}
			if !inWindow {
				return Version{}, hiddenVersionError()
			}
		}
		return v, nil
	})
}

// PurgeSpace hard-deletes every canvas of one Space and its versions, then fences the Space so no canvas can be created
// for it again. It runs on the unscoped pool, so RLS does not apply: every statement filters by org_id as well as
// space_ref. A replay finds nothing and returns zero counts.
func (r *Repo) PurgeSpace(ctx context.Context, orgID, spaceRef string) (PurgeCounts, error) {
	if err := validateScope(orgID, spaceRef); err != nil {
		return PurgeCounts{}, err
	}
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return PurgeCounts{}, fmt.Errorf("begin canvas space purge: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }() // no-op once Commit succeeds

	// The same lock writeTx takes: a write that holds it finishes first, and a later create finds the fence below.
	if _, err := tx.Exec(ctx, `SELECT pg_advisory_xact_lock(hashtextextended($1, 0))`, spaceLockKey(orgID, spaceRef)); err != nil {
		return PurgeCounts{}, fmt.Errorf("lock canvas space for purge: %w", err)
	}

	versions, err := tx.Exec(ctx, `DELETE FROM space_canvas_versions
		WHERE org_id = $1
		  AND canvas_id IN (SELECT canvas_id FROM space_canvases WHERE org_id = $1 AND space_ref = $2)`,
		orgID, spaceRef)
	if err != nil {
		return PurgeCounts{}, fmt.Errorf("purge canvas versions: %w", err)
	}
	canvases, err := tx.Exec(ctx, `DELETE FROM space_canvases WHERE org_id = $1 AND space_ref = $2`, orgID, spaceRef)
	if err != nil {
		return PurgeCounts{}, fmt.Errorf("purge canvases: %w", err)
	}
	// The fence stops a create decided before the deletion from landing after it. It is written even when the Space never
	// had a canvas. A replay finds it already present.
	if _, err := tx.Exec(ctx, `INSERT INTO space_canvas_purges (org_id, space_ref) VALUES ($1, $2) ON CONFLICT DO NOTHING`, orgID, spaceRef); err != nil {
		return PurgeCounts{}, fmt.Errorf("record canvas space fence: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return PurgeCounts{}, fmt.Errorf("commit canvas space purge: %w", err)
	}
	return PurgeCounts{CanvasesPurged: canvases.RowsAffected(), VersionsPurged: versions.RowsAffected()}, nil
}

// PurgeOrgTx hard-deletes every canvas, version and Space fence of one org inside the caller's transaction (HardPurgeByOrg),
// versions first. Every statement filters by org_id. The caller runs them unscoped, as HardPurgeByOrg does.
func PurgeOrgTx(ctx context.Context, tx pgx.Tx, orgID string) (PurgeCounts, error) {
	if strings.TrimSpace(orgID) == "" {
		return PurgeCounts{}, errors.New("canvas: an org id is required")
	}
	versions, err := tx.Exec(ctx, `DELETE FROM space_canvas_versions WHERE org_id = $1`, orgID)
	if err != nil {
		return PurgeCounts{}, fmt.Errorf("purge canvas versions: %w", err)
	}
	canvases, err := tx.Exec(ctx, `DELETE FROM space_canvases WHERE org_id = $1`, orgID)
	if err != nil {
		return PurgeCounts{}, fmt.Errorf("purge canvases: %w", err)
	}
	if _, err := tx.Exec(ctx, `DELETE FROM space_canvas_purges WHERE org_id = $1`, orgID); err != nil {
		return PurgeCounts{}, fmt.Errorf("purge canvas space fences: %w", err)
	}
	return PurgeCounts{CanvasesPurged: canvases.RowsAffected(), VersionsPurged: versions.RowsAffected()}, nil
}

// writeOp is one save or restore after validation.
type writeOp struct {
	op          string
	orgID       string
	spaceRef    string
	canvasID    string // the canvas the decision named; "" for a create
	expected    *string
	content     string // save: the new text
	restoreFrom string // restore: the version whose text is copied
	floor       Floor  // restore: the member's history window
	summary     *string
	idemKey     string
	author      Author
	prov        Provenance
}

// write runs one save or restore in the org scope. Any typed refusal rolls the transaction back.
func (r *Repo) write(ctx context.Context, w writeOp) (WriteResult, error) {
	return orgscope.InOrgScope(ctx, r.pool, w.orgID, func(ctx context.Context, tx pgx.Tx) (WriteResult, error) {
		return writeTx(ctx, tx, w)
	})
}

// writeTx runs the write algorithm. The Space's advisory lock is taken first, then the canvas row, so writers to one Space
// are serialised and a purge cannot interleave with them. Every check after that reads the head under the locks.
func writeTx(ctx context.Context, tx pgx.Tx, w writeOp) (WriteResult, error) {
	if _, err := tx.Exec(ctx, `SELECT pg_advisory_xact_lock(hashtextextended($1, 0))`, spaceLockKey(w.orgID, w.spaceRef)); err != nil {
		return WriteResult{}, fmt.Errorf("lock canvas space: %w", err)
	}
	cv, found, err := findActiveCanvas(ctx, tx, w.orgID, w.spaceRef, true)
	if err != nil {
		return WriteResult{}, err
	}
	if err := bindDecidedCanvas(ctx, tx, w, cv, found); err != nil {
		return WriteResult{}, err
	}

	// The text this write stores: the save's own text, or the text of the restored version.
	content := w.content
	var digest string
	var restoredFrom *string
	if w.op == opRestore {
		src, err := loadVersion(ctx, tx, w.orgID, cv.CanvasID, w.restoreFrom, true)
		if errors.Is(err, pgx.ErrNoRows) {
			return WriteResult{}, &Error{Code: CodeVersionNotFound, Message: "the version is not in this canvas"}
		}
		if err != nil {
			return WriteResult{}, fmt.Errorf("load restore source: %w", err)
		}
		// A version written before the member's window cannot be restored: it would show its text as the new head (D21m).
		// The current head is exempt; restoring it is unchanged. Checked under the Space lock, after not-found and before the
		// head compare-and-swap.
		if w.floor.Set() {
			inWindow, _, err := versionInWindow(ctx, tx, w.orgID, cv.CanvasID, w.restoreFrom, w.floor)
			if err != nil {
				return WriteResult{}, err
			}
			if !inWindow {
				return WriteResult{}, hiddenVersionError()
			}
		}
		content = src.Content
		digest = src.ContentSHA256
		from := w.restoreFrom
		restoredFrom = &from
	} else {
		digest = ContentDigest(content)
	}

	created := false
	if !found {
		var insertedID string
		insErr := tx.QueryRow(ctx, `
			INSERT INTO space_canvases
				(canvas_id, org_id, space_ref, state, head_version_id, head_version_no, created_by, created_at, updated_at)
			VALUES ($1, $2, $3, 'active', NULL, 0, $4, now(), now())
			ON CONFLICT DO NOTHING
			RETURNING canvas_id`,
			uuid.NewString(), w.orgID, w.spaceRef, w.author.ID).Scan(&insertedID)
		switch {
		case insErr == nil:
			created = true
			cv = Canvas{CanvasID: insertedID, SpaceRef: w.spaceRef, State: "active"}
		case errors.Is(insErr, pgx.ErrNoRows):
			// A concurrent creator committed first. Continue against its canvas: a replay of this same write still
			// deduplicates below, and any other write loses the head compare-and-swap.
			cv, found, err = findActiveCanvas(ctx, tx, w.orgID, w.spaceRef, true)
			if err != nil {
				return WriteResult{}, err
			}
			if !found {
				// The competing canvas is gone again (no writer can purge under the Space lock, so this is defensive).
				return WriteResult{}, canvasNotFound()
			}
		default:
			return WriteResult{}, fmt.Errorf("create canvas: %w", insErr)
		}
	}

	canvasID := cv.CanvasID
	idem := idempotencyHash(w.orgID, canvasID, w.op, w.author, w.idemKey)

	var head *Version
	if !created && cv.HeadVersionID != nil {
		h, err := loadVersion(ctx, tx, w.orgID, canvasID, *cv.HeadVersionID, false)
		if err != nil {
			return WriteResult{}, fmt.Errorf("load canvas head: %w", err)
		}
		head = &h
	}

	// Idempotency: a replay under the same key returns the version that key stored, and never a second copy. The same key
	// with different content is a conflict.
	if !created {
		var prevID, prevDigest string
		err := tx.QueryRow(ctx, `
			SELECT version_id, content_sha256 FROM space_canvas_versions
			WHERE org_id = $1 AND canvas_id = $2 AND idempotency_hash = $3`,
			w.orgID, canvasID, idem).Scan(&prevID, &prevDigest)
		switch {
		case err == nil:
			if prevDigest != digest {
				return WriteResult{}, &Error{Code: CodeIdempotencyKeyConflict, Message: "the idempotency key was used for different content"}
			}
			prev, err := loadVersion(ctx, tx, w.orgID, canvasID, prevID, false)
			if err != nil {
				return WriteResult{}, fmt.Errorf("load deduplicated version: %w", err)
			}
			return WriteResult{CanvasID: canvasID, Version: prev, Deduplicated: true, HeadVersionID: headVersionID(cv)}, nil
		case !errors.Is(err, pgx.ErrNoRows):
			return WriteResult{}, fmt.Errorf("look up idempotency key: %w", err)
		}
	}

	// Compare-and-swap: the caller must hold the current head. A refused write stores nothing.
	if !expectsHead(head, w.expected) {
		return WriteResult{}, &Error{Code: CodeHeadMoved, Message: "the canvas head moved since you opened it", Head: headRefOf(head)}
	}

	// Unchanged: the content equals the head, so nothing is written.
	if head != nil && head.ContentSHA256 == digest {
		return WriteResult{CanvasID: canvasID, Unchanged: true, HeadVersionID: head.VersionID}, nil
	}

	if !created {
		if err := checkWriteLimits(ctx, tx, canvasID, w.author); err != nil {
			return WriteResult{}, err
		}
	}

	changeKind := changeEdit
	switch {
	case w.op == opRestore:
		changeKind = changeRestore
	case created:
		changeKind = changeCreate
	}
	versionNo := cv.HeadVersionNo + 1
	parent := cv.HeadVersionID
	var onBehalf, runID *string
	if w.author.Kind == AuthorAgent {
		behalf := w.author.OnBehalfOf
		run := w.author.RunID
		onBehalf = &behalf
		runID = &run
	}

	versionID := uuid.NewString()
	var createdAt time.Time
	if err := tx.QueryRow(ctx, `
		INSERT INTO space_canvas_versions (
			version_id, canvas_id, org_id, version_no, parent_version_id, content, content_sha256, content_bytes,
			author_kind, author_id, on_behalf_of, run_id, change_kind, restored_from_version_id, summary,
			decision_ref, authority_revision, audience_revision, idempotency_hash)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19)
		RETURNING created_at`,
		versionID, canvasID, w.orgID, versionNo, parent, content, digest, len(content),
		w.author.Kind, w.author.ID, onBehalf, runID, changeKind, restoredFrom, w.summary,
		w.prov.DecisionRef, w.prov.AuthorityRevision, w.prov.AudienceRevision, idem,
	).Scan(&createdAt); err != nil {
		return WriteResult{}, fmt.Errorf("insert canvas version: %w", err)
	}

	tag, err := tx.Exec(ctx, `
		UPDATE space_canvases SET head_version_id = $1, head_version_no = $2, updated_at = now()
		WHERE canvas_id = $3 AND org_id = $4`,
		versionID, versionNo, canvasID, w.orgID)
	if err != nil {
		return WriteResult{}, fmt.Errorf("advance canvas head: %w", err)
	}
	if tag.RowsAffected() != 1 {
		return WriteResult{}, fmt.Errorf("advance canvas head: %d rows updated", tag.RowsAffected())
	}

	return WriteResult{
		CanvasID: canvasID,
		Version: Version{
			VersionID:             versionID,
			CanvasID:              canvasID,
			VersionNo:             versionNo,
			ParentVersionID:       parent,
			AuthorKind:            w.author.Kind,
			AuthorID:              w.author.ID,
			OnBehalfOf:            onBehalf,
			RunID:                 runID,
			ChangeKind:            changeKind,
			RestoredFromVersionID: restoredFrom,
			Summary:               w.summary,
			ContentBytes:          len(content),
			ContentSHA256:         digest,
			CreatedAt:             createdAt,
		},
		Inserted:      true,
		HeadVersionID: versionID,
	}, nil
}

// bindDecidedCanvas refuses a write that would land anywhere but the canvas its Control decision named. Called under the
// Space lock and the canvas row lock, so no purge or concurrent writer can change the answer before the write. An empty
// CanvasID is a create: the Space must still have no canvas, and a purged Space is fenced and cannot take one. A save or
// restore must find exactly its canvas. Every refusal stores nothing.
//
// The two refusals name two situations (D21n). canvas_not_found: the Space holds no canvas the write could land in, because none
// was created, it was purged, or the Space is fenced. canvas_head_moved: a canvas exists, but it is not the head the caller opened
// (another write moved the head, a create raced another create, or the canvas was replaced). Only head_moved names a head.
func bindDecidedCanvas(ctx context.Context, tx pgx.Tx, w writeOp, cv Canvas, found bool) error {
	if w.canvasID == "" {
		if found {
			return headMovedAt(ctx, tx, w.orgID, cv)
		}
		if w.expected != nil {
			// The editor opened a canvas that is gone: never created here, or purged.
			return canvasNotFound()
		}
		fenced, err := spaceCanvasFenced(ctx, tx, w.orgID, w.spaceRef)
		if err != nil {
			return err
		}
		if fenced {
			return canvasNotFound()
		}
		return nil
	}
	if !found {
		// The canvas the decision named is gone, for a save and a restore alike.
		return canvasNotFound()
	}
	if cv.CanvasID != w.canvasID {
		return headMovedAt(ctx, tx, w.orgID, cv)
	}
	return nil
}

// canvasNotFound is the typed refusal of a Space that holds no canvas.
func canvasNotFound() *Error {
	return &Error{Code: CodeCanvasNotFound, Message: "the Space has no canvas"}
}

// headMovedAt is the typed head-moved refusal, naming the canvas's current head.
func headMovedAt(ctx context.Context, tx pgx.Tx, orgID string, cv Canvas) error {
	var head *Version
	if cv.HeadVersionID != nil {
		h, err := loadVersion(ctx, tx, orgID, cv.CanvasID, *cv.HeadVersionID, false)
		if err != nil {
			return fmt.Errorf("load canvas head: %w", err)
		}
		head = &h
	}
	return &Error{Code: CodeHeadMoved, Message: "the canvas head moved since you opened it", Head: headRefOf(head)}
}

// spaceCanvasFenced reports whether the Space was purged. A fenced Space never takes a new canvas.
func spaceCanvasFenced(ctx context.Context, tx pgx.Tx, orgID, spaceRef string) (bool, error) {
	var fenced bool
	if err := tx.QueryRow(ctx,
		`SELECT EXISTS (SELECT 1 FROM space_canvas_purges WHERE org_id = $1 AND space_ref = $2)`,
		orgID, spaceRef).Scan(&fenced); err != nil {
		return false, fmt.Errorf("read canvas space fence: %w", err)
	}
	return fenced, nil
}

// spaceLockKey names the advisory lock that serialises the writes to one Space and the purge of that Space. The database
// hashes the key, as the retrieval engine does for its Space locks.
func spaceLockKey(orgID, spaceRef string) string {
	return "space_canvas:" + orgID + ":" + spaceRef
}

// checkWriteLimits applies the per-run and per-author write limits inside the write transaction. The counts use the
// (canvas_id, created_at) index.
func checkWriteLimits(ctx context.Context, tx pgx.Tx, canvasID string, author Author) error {
	if author.Kind == AuthorAgent {
		var runWrites int
		if err := tx.QueryRow(ctx,
			`SELECT count(*) FROM space_canvas_versions WHERE canvas_id = $1 AND run_id = $2`,
			canvasID, author.RunID).Scan(&runWrites); err != nil {
			return fmt.Errorf("count run writes: %w", err)
		}
		if runWrites >= MaxWritesPerAgentRun {
			return &Error{Code: CodeRunWriteLimit, Message: "this agent run has reached its write limit for the canvas"}
		}
	}
	var authorWrites int
	if err := tx.QueryRow(ctx, `
		SELECT count(*) FROM space_canvas_versions
		WHERE canvas_id = $1 AND author_kind = $2 AND author_id = $3
		  AND created_at > now() - interval '1 hour'`,
		canvasID, author.Kind, author.ID).Scan(&authorWrites); err != nil {
		return fmt.Errorf("count author writes: %w", err)
	}
	if authorWrites >= MaxWritesPerAuthorPerHour {
		return &Error{Code: CodeRateLimited, Message: "too many writes to this canvas in the last hour"}
	}
	return nil
}

// expectsHead is the compare-and-swap test: no canvas expects no head, and a canvas expects exactly its head.
func expectsHead(head *Version, expected *string) bool {
	if head == nil {
		return expected == nil
	}
	return expected != nil && *expected == head.VersionID
}

func headRefOf(v *Version) *HeadRef {
	if v == nil {
		return nil
	}
	return &HeadRef{VersionID: v.VersionID, VersionNo: v.VersionNo, AuthorKind: v.AuthorKind, CreatedAt: v.CreatedAt}
}

func headVersionID(c Canvas) string {
	if c.HeadVersionID == nil {
		return ""
	}
	return *c.HeadVersionID
}

// validateScope requires an org and a Space ref with no padding. The org comes from verified claims. The Space ref
// comes from the route, and the HTTP layer checks it against the decision.
func validateScope(orgID, spaceRef string) error {
	if strings.TrimSpace(orgID) == "" {
		return errors.New("canvas: an org id is required")
	}
	if spaceRef == "" || len(spaceRef) > MaxSpaceRefBytes || strings.TrimSpace(spaceRef) != spaceRef {
		return invalidRequest("space_ref is not valid")
	}
	return nil
}

// validateAuthor checks the stamp the HTTP layer derived from the decision envelope. A malformed stamp is a caller bug,
// so it is not a typed client error.
func validateAuthor(a Author) error {
	switch a.Kind {
	case AuthorUser:
		if strings.TrimSpace(a.ID) == "" || a.OnBehalfOf != "" || a.RunID != "" {
			return errors.New("canvas: a user author needs a subject and no run fields")
		}
	case AuthorAgent:
		if a.RunID == "" || a.OnBehalfOf == "" || a.ID != "run:"+a.RunID {
			return errors.New("canvas: an agent author needs a run, the invoking subject and the id run:<run_id>")
		}
	default:
		return errors.New("canvas: unknown author kind")
	}
	return nil
}

// validateProvenance requires the decision reference and both revisions a write is bound to.
func validateProvenance(p Provenance) error {
	if strings.TrimSpace(p.DecisionRef) == "" || p.AuthorityRevision <= 0 || p.AudienceRevision <= 0 {
		return errors.New("canvas: a write needs the verified decision provenance")
	}
	return nil
}
