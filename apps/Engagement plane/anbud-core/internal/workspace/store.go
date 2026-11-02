// Package workspace keeps what an organisation does with the tender list: its company profile, where each
// tender stands in its pipeline, and the searches it saves. Every query runs in a transaction that names the
// organisation, which is what the row level policy of migration 0009 compares each row with, so one
// organisation's rows are invisible to every other, whatever the caller passes.
package workspace

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"regexp"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/noticeid"
)

// Stages are the pipeline stages an organisation can give a tender: the ids the app shows.
var Stages = map[string]bool{"inbox": true, "review": true, "working": true, "sent": true, "hidden": true}

const (
	maxProfileBytes = 16384
	maxQueryBytes   = 8192
	maxNameRunes    = 120
	maxSearches     = 50
)

var (
	// ErrInvalid refuses a value the store will not keep. The message names the rule, never the value.
	ErrInvalid = errors.New("workspace: invalid value")
	// ErrNotFound means no row of the organisation has that id.
	ErrNotFound = errors.New("workspace: not found")
	// ErrTooMany refuses a saved search beyond the organisation's limit.
	ErrTooMany = errors.New("workspace: too many saved searches")

	orgPattern = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$`)
)

// Store reads and writes one organisation's workspace.
type Store struct {
	pool *pgxpool.Pool
	now  func() time.Time
}

// New returns a Store over pool.
func New(pool *pgxpool.Pool) *Store {
	return &Store{pool: pool, now: time.Now}
}

// Profile is the stored company profile of an organisation, as the app sent it.
type Profile struct {
	Profile   json.RawMessage `json:"profile"`
	UpdatedAt time.Time       `json:"updated_at"`
}

// PipelineItem is where one tender stands in an organisation's pipeline.
type PipelineItem struct {
	NoticeID  string    `json:"notice_id"`
	Stage     string    `json:"stage"`
	UpdatedAt time.Time `json:"updated_at"`
}

// SavedSearch is a search an organisation saved: a name and the filters it ran with.
type SavedSearch struct {
	ID        int64           `json:"id"`
	Name      string          `json:"name"`
	Query     json.RawMessage `json:"query"`
	CreatedAt time.Time       `json:"created_at"`
}

// scoped runs fn in a transaction whose session names org.
func (s *Store) scoped(ctx context.Context, org string, fn func(tx pgx.Tx) error) error {
	if !orgPattern.MatchString(org) {
		return fmt.Errorf("%w: the organisation id has an invalid shape", ErrInvalid)
	}
	tx, err := s.pool.BeginTx(ctx, pgx.TxOptions{IsoLevel: pgx.ReadCommitted})
	if err != nil {
		return fmt.Errorf("workspace: begin: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	if _, err := tx.Exec(ctx, `SELECT set_config('anbud.org_id', $1, true)`, org); err != nil {
		return fmt.Errorf("workspace: set the organisation: %w", err)
	}
	if err := fn(tx); err != nil {
		return err
	}
	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("workspace: commit: %w", err)
	}
	return nil
}

// GetProfile returns the organisation's profile, or ErrNotFound when it has never saved one.
func (s *Store) GetProfile(ctx context.Context, org string) (Profile, error) {
	var p Profile
	err := s.scoped(ctx, org, func(tx pgx.Tx) error {
		return tx.QueryRow(ctx, `SELECT profile, updated_at FROM tenant.workspace_profiles WHERE org_id = $1`, org).Scan(&p.Profile, &p.UpdatedAt)
	})
	if errors.Is(err, pgx.ErrNoRows) {
		return Profile{}, ErrNotFound
	}
	if err != nil {
		return Profile{}, fmt.Errorf("workspace: read profile: %w", err)
	}
	return p, nil
}

// PutProfile saves the organisation's profile. The body must be a JSON object within the size limit.
func (s *Store) PutProfile(ctx context.Context, org, by string, profile json.RawMessage) (Profile, error) {
	if err := checkProfile(profile, by); err != nil {
		return Profile{}, err
	}
	var p Profile
	err := s.scoped(ctx, org, func(tx pgx.Tx) error {
		return tx.QueryRow(ctx, `
INSERT INTO tenant.workspace_profiles (org_id, profile, updated_at, updated_by)
VALUES ($1, $2, $3, $4)
ON CONFLICT (org_id) DO UPDATE SET profile = EXCLUDED.profile, updated_at = EXCLUDED.updated_at, updated_by = EXCLUDED.updated_by
RETURNING profile, updated_at`, org, []byte(profile), s.now().UTC(), by).Scan(&p.Profile, &p.UpdatedAt)
	})
	if err != nil {
		return Profile{}, fmt.Errorf("workspace: save profile: %w", err)
	}
	return p, nil
}

// PutProfileIfAbsent saves a profile only when the organisation has none, and reports whether it did.
// A suggestion uses it, so a profile the organisation saved is never replaced by one drawn from its wins.
func (s *Store) PutProfileIfAbsent(ctx context.Context, org, by string, profile json.RawMessage) (Profile, bool, error) {
	if err := checkProfile(profile, by); err != nil {
		return Profile{}, false, err
	}
	var p Profile
	err := s.scoped(ctx, org, func(tx pgx.Tx) error {
		return tx.QueryRow(ctx, `
INSERT INTO tenant.workspace_profiles (org_id, profile, updated_at, updated_by)
VALUES ($1, $2, $3, $4)
ON CONFLICT (org_id) DO NOTHING
RETURNING profile, updated_at`, org, []byte(profile), s.now().UTC(), by).Scan(&p.Profile, &p.UpdatedAt)
	})
	if errors.Is(err, pgx.ErrNoRows) {
		return Profile{}, false, nil
	}
	if err != nil {
		return Profile{}, false, fmt.Errorf("workspace: save suggested profile: %w", err)
	}
	return p, true, nil
}

// checkProfile refuses what the store will not keep: a body that is not an object, one over the size
// limit, or a user id of the wrong shape.
func checkProfile(profile json.RawMessage, by string) error {
	if !isObject(profile) {
		return fmt.Errorf("%w: the profile must be a JSON object", ErrInvalid)
	}
	if len(profile) > maxProfileBytes {
		return fmt.Errorf("%w: the profile is larger than %d bytes", ErrInvalid, maxProfileBytes)
	}
	if !validActor(by) {
		return fmt.Errorf("%w: the user id has an invalid shape", ErrInvalid)
	}
	return nil
}

// ListPipeline returns every tender the organisation has put in a stage, newest change first.
func (s *Store) ListPipeline(ctx context.Context, org string) ([]PipelineItem, error) {
	var out []PipelineItem
	err := s.scoped(ctx, org, func(tx pgx.Tx) error {
		rows, err := tx.Query(ctx, `SELECT notice_id, stage, updated_at FROM tenant.pipeline_items WHERE org_id=$1 ORDER BY updated_at DESC, notice_id`, org)
		if err != nil {
			return err
		}
		defer rows.Close()
		for rows.Next() {
			var it PipelineItem
			if err := rows.Scan(&it.NoticeID, &it.Stage, &it.UpdatedAt); err != nil {
				return err
			}
			out = append(out, it)
		}
		return rows.Err()
	})
	if err != nil {
		return nil, fmt.Errorf("workspace: read pipeline: %w", err)
	}
	if out == nil {
		out = []PipelineItem{}
	}
	return out, nil
}

// SetStage puts a tender in a stage of the organisation's pipeline.
func (s *Store) SetStage(ctx context.Context, org, by, noticeID, stage string) (PipelineItem, error) {
	if !noticeid.Valid(noticeID) {
		return PipelineItem{}, fmt.Errorf("%w: the notice id has an invalid shape", ErrInvalid)
	}
	if !Stages[stage] {
		return PipelineItem{}, fmt.Errorf("%w: the stage is not one of the pipeline's", ErrInvalid)
	}
	if !validActor(by) {
		return PipelineItem{}, fmt.Errorf("%w: the user id has an invalid shape", ErrInvalid)
	}
	var it PipelineItem
	err := s.scoped(ctx, org, func(tx pgx.Tx) error {
		return tx.QueryRow(ctx, `
INSERT INTO tenant.pipeline_items (org_id, notice_id, stage, updated_at, updated_by)
VALUES ($1, $2, $3, $4, $5)
ON CONFLICT (org_id, notice_id) DO UPDATE SET stage = EXCLUDED.stage, updated_at = EXCLUDED.updated_at, updated_by = EXCLUDED.updated_by
RETURNING notice_id, stage, updated_at`, org, noticeID, stage, s.now().UTC(), by).Scan(&it.NoticeID, &it.Stage, &it.UpdatedAt)
	})
	if err != nil {
		return PipelineItem{}, fmt.Errorf("workspace: set stage: %w", err)
	}
	return it, nil
}

// ListSearches returns the organisation's saved searches, oldest first.
func (s *Store) ListSearches(ctx context.Context, org string) ([]SavedSearch, error) {
	var out []SavedSearch
	err := s.scoped(ctx, org, func(tx pgx.Tx) error {
		rows, err := tx.Query(ctx, `SELECT id, name, query, created_at FROM tenant.saved_searches ORDER BY id`)
		if err != nil {
			return err
		}
		defer rows.Close()
		for rows.Next() {
			var q SavedSearch
			if err := rows.Scan(&q.ID, &q.Name, &q.Query, &q.CreatedAt); err != nil {
				return err
			}
			out = append(out, q)
		}
		return rows.Err()
	})
	if err != nil {
		return nil, fmt.Errorf("workspace: read saved searches: %w", err)
	}
	if out == nil {
		out = []SavedSearch{}
	}
	return out, nil
}

// SaveSearch stores a search under a name. An organisation keeps at most maxSearches.
func (s *Store) SaveSearch(ctx context.Context, org, by, name string, query json.RawMessage) (SavedSearch, error) {
	if n := len([]rune(name)); n < 1 || n > maxNameRunes {
		return SavedSearch{}, fmt.Errorf("%w: the name must be between 1 and %d characters", ErrInvalid, maxNameRunes)
	}
	if !isObject(query) || len(query) > maxQueryBytes {
		return SavedSearch{}, fmt.Errorf("%w: the search must be a JSON object of at most %d bytes", ErrInvalid, maxQueryBytes)
	}
	if !validActor(by) {
		return SavedSearch{}, fmt.Errorf("%w: the user id has an invalid shape", ErrInvalid)
	}
	var q SavedSearch
	err := s.scoped(ctx, org, func(tx pgx.Tx) error {
		var n int
		if err := tx.QueryRow(ctx, `SELECT count(*) FROM tenant.saved_searches`).Scan(&n); err != nil {
			return err
		}
		if n >= maxSearches {
			return ErrTooMany
		}
		return tx.QueryRow(ctx, `
INSERT INTO tenant.saved_searches (org_id, name, query, created_at, created_by)
VALUES ($1, $2, $3, $4, $5)
RETURNING id, name, query, created_at`, org, name, []byte(query), s.now().UTC(), by).Scan(&q.ID, &q.Name, &q.Query, &q.CreatedAt)
	})
	if errors.Is(err, ErrTooMany) {
		return SavedSearch{}, fmt.Errorf("%w: at most %d are kept; delete one first", ErrTooMany, maxSearches)
	}
	if err != nil {
		return SavedSearch{}, fmt.Errorf("workspace: save search: %w", err)
	}
	return q, nil
}

// DeleteSearch removes one saved search of the organisation.
func (s *Store) DeleteSearch(ctx context.Context, org string, id int64) error {
	var rows int64
	err := s.scoped(ctx, org, func(tx pgx.Tx) error {
		tag, err := tx.Exec(ctx, `DELETE FROM tenant.saved_searches WHERE id = $1`, id)
		rows = tag.RowsAffected()
		return err
	})
	if err != nil {
		return fmt.Errorf("workspace: delete search: %w", err)
	}
	if rows == 0 {
		return ErrNotFound
	}
	return nil
}

func isObject(raw json.RawMessage) bool {
	var v map[string]json.RawMessage
	return json.Unmarshal(raw, &v) == nil && v != nil
}

// validActor is a user id: the gateway's own shape, which is a short token and never a name.
func validActor(id string) bool {
	return id != "" && len(id) <= 128 && orgPattern.MatchString(id)
}

// TrackedNoticeIDs returns explicit follow-up work, independent of current matches.
func (s *Store) TrackedNoticeIDs(ctx context.Context, org string) ([]string, error) {
	items, err := s.ListPipeline(ctx, org)
	if err != nil {
		return nil, err
	}
	ids := []string{}
	for _, item := range items {
		if item.Stage == "review" || item.Stage == "working" || item.Stage == "sent" || item.Stage == "hidden" {
			ids = append(ids, item.NoticeID)
		}
	}
	return ids, nil
}
