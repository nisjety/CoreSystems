package procedures

import (
	"context"
	"errors"
	"fmt"
	"math"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

// ErrNotFound means no such procedure, version or tag for this owner. A procedure that exists but
// belongs to someone else answers the same way, so a caller cannot probe for other people's recipes.
var ErrNotFound = errors.New("procedure not found")

// ErrInvalidRef means a version reference names both a version and a tag, or a negative version.
var ErrInvalidRef = errors.New("a version reference names a version or a tag, not both")

// ErrVersionLimit means the procedure already has MaxVersions versions. Versions are immutable and
// are never pruned, so the limit is how a runaway writer is stopped.
var ErrVersionLimit = errors.New("procedure has reached its version limit")

// ErrTagLimit means the procedure already has MaxTags tags.
var ErrTagLimit = errors.New("procedure has reached its tag limit")

const (
	// MaxVersions bounds the immutable versions of one procedure.
	MaxVersions = 200
	// MaxTags bounds the tags of one procedure.
	MaxTags = 32
	// DefaultListLimit and MaxListLimit bound one page of List.
	DefaultListLimit = 20
	MaxListLimit     = 100
)

// maxVersionNumber is the largest version number the INTEGER version columns hold (migration 0039). A version
// number reaches Postgres as an int4 parameter, and pgx refuses to encode a larger one, which the caller would
// see as a store failure. No stored version has a larger number, so such a number is answered as not found.
const maxVersionNumber = math.MaxInt32

// scopeUser is the only scope today. Sharing a recipe is ADR 8 and is not built; the migration's
// CHECK admits no other value.
const scopeUser = "user"

// database is the narrow slice of pgxpool.Pool this store uses, so the owner scoping and the
// decision logic are unit-testable against a stub and only the transaction paths need a live
// Postgres (the same shape as watch.Store).
type database interface {
	Exec(context.Context, string, ...any) (pgconn.CommandTag, error)
	Query(context.Context, string, ...any) (pgx.Rows, error)
	QueryRow(context.Context, string, ...any) pgx.Row
	Begin(context.Context) (pgx.Tx, error)
}

// execer is what a pool and a transaction share, for the statements used on both.
type execer interface {
	Exec(context.Context, string, ...any) (pgconn.CommandTag, error)
}

// Owner is the verified identity a request runs as. OrgID and UserID come from the verified
// token (the handler's verifiedOrganizationID and verifiedActorID), never from a request header or
// body field. Every query is scoped by both.
type Owner struct {
	OrgID  string
	UserID string
}

func (o Owner) check() error {
	if o.OrgID == "" || o.UserID == "" {
		return errors.New("org_id and owner user id are required")
	}
	return nil
}

// Procedure is one saved recipe: its metadata and which version is newest.
type Procedure struct {
	ID            string
	OrgID         string
	OwnerUserID   string
	Scope         string
	Name          string
	Description   string
	SourceRunID   string
	LatestVersion int
	CreatedAt     time.Time
	UpdatedAt     time.Time
	// Tags are the procedure's tags, by name. Filled by Get and left nil by List.
	Tags []Tag
}

// Tag points a name at one version of a procedure.
type Tag struct {
	Tag     string
	Version int
}

// Version is one immutable version of a procedure.
type Version struct {
	ProcedureID string
	OrgID       string
	Version     int
	Digest      string
	// Body is the canonical BrowserProcedure v2 JSON that was validated and digested.
	Body      []byte
	StepCount int
	SlotNames []string
	CreatedBy string
	CreatedAt time.Time
}

// Ref names a version of a procedure: an explicit number, a tag, or (the zero value) the latest.
type Ref struct {
	Version int
	Tag     string
}

// Page is one page of List. NextAfterID is the cursor for the next page, empty on the last one.
type Page struct {
	Items       []Procedure
	NextAfterID string
}

// Store is the durable procedure registry backed by procedures, procedure_versions and
// procedure_tags (capability-core migration 0039).
type Store struct {
	pool  database
	nowFn func() time.Time
	newID func() string
}

// NewStore constructs a store over an already-configured pool.
func NewStore(pool database) (*Store, error) {
	if pool == nil {
		return nil, errors.New("a procedure store requires a database pool")
	}
	return &Store{
		pool:  pool,
		nowFn: func() time.Time { return time.Now().UTC() },
		newID: func() string { return "proc_" + uuid.NewString() },
	}, nil
}

const procedureColumns = `id, org_id, owner_user_id, scope, name, description, source_run_id, ` +
	`latest_version, created_at, updated_at`

const versionColumns = `pv.procedure_id, pv.org_id, pv.version, pv.digest, pv.body, pv.step_count, ` +
	`pv.slot_names, pv.created_by, pv.created_at`

// Create records a new procedure with its first version (version 1). The body is validated first;
// a refusal is a *ValidationError and nothing is written.
func (s *Store) Create(ctx context.Context, who Owner, meta Meta, body []byte) (*Procedure, *Version, error) {
	if err := who.check(); err != nil {
		return nil, nil, err
	}
	meta, err := NormalizeMeta(meta)
	if err != nil {
		return nil, nil, err
	}
	summary, err := Validate(body)
	if err != nil {
		return nil, nil, err
	}
	now := s.nowFn()
	p := Procedure{
		ID: s.newID(), OrgID: who.OrgID, OwnerUserID: who.UserID, Scope: scopeUser,
		Name: meta.Name, Description: meta.Description, SourceRunID: meta.SourceRunID,
		LatestVersion: 1, CreatedAt: now, UpdatedAt: now,
	}

	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return nil, nil, fmt.Errorf("create procedure: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	if _, err := tx.Exec(ctx, `
		INSERT INTO procedures (id, org_id, owner_user_id, scope, name, description, source_run_id,
		                        latest_version, created_at, updated_at)
		VALUES ($1, $2, $3, $4, $5, $6, $7, 1, $8, $8)
	`, p.ID, p.OrgID, p.OwnerUserID, p.Scope, p.Name, p.Description, p.SourceRunID, now); err != nil {
		return nil, nil, fmt.Errorf("create procedure: %w", err)
	}
	v, err := insertVersion(ctx, tx, who, p.ID, 1, summary, now)
	if err != nil {
		return nil, nil, err
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, nil, fmt.Errorf("create procedure: %w", err)
	}
	return &p, v, nil
}

// AddVersion appends the next immutable version to a procedure the caller owns. The procedure row
// is locked for the read of latest_version, so two writers get consecutive numbers, never the same
// one. Earlier versions are untouched.
func (s *Store) AddVersion(ctx context.Context, who Owner, id string, body []byte) (*Version, error) {
	if err := who.check(); err != nil {
		return nil, err
	}
	if !ValidID(id) {
		return nil, ErrNotFound
	}
	summary, err := Validate(body)
	if err != nil {
		return nil, err
	}
	now := s.nowFn()

	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("add procedure version: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	var latest int
	err = tx.QueryRow(ctx, `
		SELECT latest_version FROM procedures
		WHERE id = $1 AND org_id = $2 AND owner_user_id = $3
		FOR UPDATE
	`, id, who.OrgID, who.UserID).Scan(&latest)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, fmt.Errorf("add procedure version: %w", err)
	}
	next := latest + 1
	if next > MaxVersions {
		return nil, ErrVersionLimit
	}
	v, err := insertVersion(ctx, tx, who, id, next, summary, now)
	if err != nil {
		return nil, err
	}
	if _, err := tx.Exec(ctx, `
		UPDATE procedures SET latest_version = $2, updated_at = $3
		WHERE id = $1
	`, id, next, now); err != nil {
		return nil, fmt.Errorf("add procedure version: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, fmt.Errorf("add procedure version: %w", err)
	}
	return v, nil
}

func insertVersion(ctx context.Context, q execer, who Owner, id string, version int, sum Summary, now time.Time) (*Version, error) {
	slots := sum.SlotNames
	if slots == nil {
		slots = []string{}
	}
	if _, err := q.Exec(ctx, `
		INSERT INTO procedure_versions (procedure_id, org_id, version, digest, body, step_count,
		                                slot_names, created_by, created_at)
		VALUES ($1, $2, $3, $4, $5::jsonb, $6, $7, $8, $9)
	`, id, who.OrgID, version, sum.Digest, string(sum.Canonical), sum.StepCount, slots, who.UserID, now); err != nil {
		return nil, fmt.Errorf("insert procedure version: %w", err)
	}
	return &Version{
		ProcedureID: id, OrgID: who.OrgID, Version: version, Digest: sum.Digest,
		Body: sum.Canonical, StepCount: sum.StepCount, SlotNames: slots,
		CreatedBy: who.UserID, CreatedAt: now,
	}, nil
}

// Get resolves one procedure the caller owns, with its tags.
func (s *Store) Get(ctx context.Context, who Owner, id string) (*Procedure, error) {
	if err := who.check(); err != nil {
		return nil, err
	}
	if !ValidID(id) {
		return nil, ErrNotFound
	}
	row := s.pool.QueryRow(ctx, `SELECT `+procedureColumns+`
		FROM procedures WHERE id = $1 AND org_id = $2 AND owner_user_id = $3`,
		id, who.OrgID, who.UserID)
	p, err := scanProcedure(row)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, fmt.Errorf("get procedure: %w", err)
	}

	rows, err := s.pool.Query(ctx, `
		SELECT t.tag, t.version
		FROM procedure_tags t
		JOIN procedures p ON p.id = t.procedure_id
		WHERE p.id = $1 AND p.org_id = $2 AND p.owner_user_id = $3
		ORDER BY t.tag
	`, id, who.OrgID, who.UserID)
	if err != nil {
		return nil, fmt.Errorf("get procedure tags: %w", err)
	}
	defer rows.Close()
	for rows.Next() {
		var t Tag
		if err := rows.Scan(&t.Tag, &t.Version); err != nil {
			return nil, fmt.Errorf("scan procedure tag: %w", err)
		}
		p.Tags = append(p.Tags, t)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("get procedure tags: %w", err)
	}
	return p, nil
}

// The three ways to pick a version. Each joins the parent row so the owner scoping is in the same
// statement as the read.
const (
	versionByNumberSQL = `SELECT ` + versionColumns + `
		FROM procedure_versions pv JOIN procedures p ON p.id = pv.procedure_id
		WHERE p.id = $1 AND p.org_id = $2 AND p.owner_user_id = $3 AND pv.version = $4`
	versionLatestSQL = `SELECT ` + versionColumns + `
		FROM procedure_versions pv JOIN procedures p ON p.id = pv.procedure_id
		WHERE p.id = $1 AND p.org_id = $2 AND p.owner_user_id = $3 AND pv.version = p.latest_version`
	versionByTagSQL = `SELECT ` + versionColumns + `
		FROM procedure_versions pv
		JOIN procedures p ON p.id = pv.procedure_id
		JOIN procedure_tags t ON t.procedure_id = pv.procedure_id AND t.version = pv.version
		WHERE p.id = $1 AND p.org_id = $2 AND p.owner_user_id = $3 AND t.tag = $4`
)

// GetVersion resolves one version of a procedure the caller owns, by number, by tag, or (the zero
// Ref) the latest. A missing procedure, version or tag, and a procedure owned by someone else, all
// answer ErrNotFound.
func (s *Store) GetVersion(ctx context.Context, who Owner, id string, ref Ref) (*Version, error) {
	if err := who.check(); err != nil {
		return nil, err
	}
	if !ValidID(id) {
		return nil, ErrNotFound
	}
	if ref.Version < 0 || (ref.Version != 0 && ref.Tag != "") {
		return nil, ErrInvalidRef
	}
	if ref.Version > maxVersionNumber {
		return nil, ErrNotFound
	}
	var row pgx.Row
	switch {
	case ref.Version > 0:
		row = s.pool.QueryRow(ctx, versionByNumberSQL, id, who.OrgID, who.UserID, ref.Version)
	case ref.Tag != "":
		if err := ValidateTag(ref.Tag); err != nil {
			return nil, ErrNotFound
		}
		row = s.pool.QueryRow(ctx, versionByTagSQL, id, who.OrgID, who.UserID, ref.Tag)
	default:
		row = s.pool.QueryRow(ctx, versionLatestSQL, id, who.OrgID, who.UserID)
	}
	var v Version
	err := row.Scan(&v.ProcedureID, &v.OrgID, &v.Version, &v.Digest, &v.Body, &v.StepCount,
		&v.SlotNames, &v.CreatedBy, &v.CreatedAt)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, fmt.Errorf("get procedure version: %w", err)
	}
	return &v, nil
}

// List returns the caller's procedures, newest first, one keyset page at a time. afterID is the
// last id of the previous page (Page.NextAfterID); an id that is not the caller's yields an empty
// page. limit defaults to DefaultListLimit and is capped at MaxListLimit.
func (s *Store) List(ctx context.Context, who Owner, limit int, afterID string) (*Page, error) {
	if err := who.check(); err != nil {
		return nil, err
	}
	if limit <= 0 {
		limit = DefaultListLimit
	}
	if limit > MaxListLimit {
		limit = MaxListLimit
	}
	rows, err := s.pool.Query(ctx, `
		SELECT `+procedureColumns+`
		FROM procedures
		WHERE org_id = $1 AND owner_user_id = $2
		  AND ($3::text = '' OR (created_at, id) < (
		        SELECT a.created_at, a.id FROM procedures a
		        WHERE a.id = $3 AND a.org_id = $1 AND a.owner_user_id = $2))
		ORDER BY created_at DESC, id DESC
		LIMIT $4
	`, who.OrgID, who.UserID, afterID, limit+1)
	if err != nil {
		return nil, fmt.Errorf("list procedures: %w", err)
	}
	defer rows.Close()
	page := &Page{}
	for rows.Next() {
		p, scanErr := scanProcedure(rows)
		if scanErr != nil {
			return nil, fmt.Errorf("scan procedure: %w", scanErr)
		}
		page.Items = append(page.Items, *p)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("list procedures: %w", err)
	}
	if len(page.Items) > limit {
		page.Items = page.Items[:limit]
		page.NextAfterID = page.Items[limit-1].ID
	}
	return page, nil
}

// SetTag points a tag at an existing version of a procedure the caller owns, creating the tag or
// moving it. A tag is the only mutable thing in this store; the versions it points at never change.
func (s *Store) SetTag(ctx context.Context, who Owner, id, tag string, version int) error {
	if err := who.check(); err != nil {
		return err
	}
	if err := ValidateTag(tag); err != nil {
		return err
	}
	if !ValidID(id) || version < 1 || version > maxVersionNumber {
		return ErrNotFound
	}
	now := s.nowFn()
	// One statement: the insert exists only when the caller owns the procedure and the version
	// exists, and a new tag only when the procedure is under its tag limit (a move never adds one).
	res, err := s.pool.Exec(ctx, `
		INSERT INTO procedure_tags (procedure_id, tag, version, updated_at)
		SELECT p.id, $4::text, pv.version, $6::timestamptz
		FROM procedures p
		JOIN procedure_versions pv ON pv.procedure_id = p.id AND pv.version = $5
		WHERE p.id = $1 AND p.org_id = $2 AND p.owner_user_id = $3
		  AND (EXISTS (SELECT 1 FROM procedure_tags e WHERE e.procedure_id = p.id AND e.tag = $4)
		       OR (SELECT count(*) FROM procedure_tags c WHERE c.procedure_id = p.id) < $7::int)
		ON CONFLICT (procedure_id, tag) DO UPDATE
		   SET version = EXCLUDED.version, updated_at = EXCLUDED.updated_at
	`, id, who.OrgID, who.UserID, tag, version, now, MaxTags)
	if err != nil {
		return fmt.Errorf("set procedure tag: %w", err)
	}
	if res.RowsAffected() > 0 {
		return nil
	}
	// Nothing written: either the procedure or version is not there for this caller, or the tag
	// limit stopped a new tag. Tell the two apart.
	var found int
	err = s.pool.QueryRow(ctx, `
		SELECT 1 FROM procedures p
		JOIN procedure_versions pv ON pv.procedure_id = p.id AND pv.version = $4
		WHERE p.id = $1 AND p.org_id = $2 AND p.owner_user_id = $3
	`, id, who.OrgID, who.UserID, version).Scan(&found)
	if errors.Is(err, pgx.ErrNoRows) {
		return ErrNotFound
	}
	if err != nil {
		return fmt.Errorf("set procedure tag: %w", err)
	}
	return ErrTagLimit
}

// DeleteTag removes one tag of a procedure the caller owns. The version it pointed at stays.
func (s *Store) DeleteTag(ctx context.Context, who Owner, id, tag string) error {
	if err := who.check(); err != nil {
		return err
	}
	if !ValidID(id) || ValidateTag(tag) != nil {
		return ErrNotFound
	}
	res, err := s.pool.Exec(ctx, `
		DELETE FROM procedure_tags t
		USING procedures p
		WHERE t.procedure_id = p.id AND p.id = $1 AND p.org_id = $2 AND p.owner_user_id = $3
		  AND t.tag = $4
	`, id, who.OrgID, who.UserID, tag)
	if err != nil {
		return fmt.Errorf("delete procedure tag: %w", err)
	}
	if res.RowsAffected() == 0 {
		return ErrNotFound
	}
	return nil
}

// Delete removes a procedure the caller owns together with all of its versions and tags. The
// versions go through the parent cascade, which the immutability trigger allows because the parent
// row is already gone; nothing else can delete a version outside erasure.
func (s *Store) Delete(ctx context.Context, who Owner, id string) error {
	if err := who.check(); err != nil {
		return err
	}
	if !ValidID(id) {
		return ErrNotFound
	}
	res, err := s.pool.Exec(ctx, `
		DELETE FROM procedures WHERE id = $1 AND org_id = $2 AND owner_user_id = $3
	`, id, who.OrgID, who.UserID)
	if err != nil {
		return fmt.Errorf("delete procedure: %w", err)
	}
	if res.RowsAffected() == 0 {
		return ErrNotFound
	}
	return nil
}

type rowScanner interface {
	Scan(dest ...any) error
}

func scanProcedure(row rowScanner) (*Procedure, error) {
	var p Procedure
	if err := row.Scan(&p.ID, &p.OrgID, &p.OwnerUserID, &p.Scope, &p.Name, &p.Description,
		&p.SourceRunID, &p.LatestVersion, &p.CreatedAt, &p.UpdatedAt); err != nil {
		return nil, err
	}
	return &p, nil
}
