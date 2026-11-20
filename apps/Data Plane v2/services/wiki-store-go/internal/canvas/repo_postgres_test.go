package canvas

// Postgres tests for the room canvas store (WS19-S1 and WS19-S8). They run only when WIKI_TEST_DATABASE_URL names a
// disposable local *_test database, and skip otherwise, like the repo package's fixtures.

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/triodelab/dataplane/shared/go/orgscope"
)

// The two migrations the canvas tables come from, in the order they apply.
const (
	canvasMigrationFile      = "20261116102141_space_canvas.sql"
	canvasFenceMigrationFile = "20261116112048_space_canvas_purge_fence.sql"
)

// canvasOutcome is one concurrent write's result, collected on the test goroutine.
type canvasOutcome struct {
	res WriteResult
	err error
}

// setupCanvasPool opens a pool over an isolated schema in the disposable test database, creates the scoped role if it is
// missing, and applies the two canvas migrations to the schema. The role gets only the grants the migrations give it, so the
// privilege checks test the migrations and not the fixture. The schema is dropped at cleanup.
func setupCanvasPool(t *testing.T) *pgxpool.Pool {
	t.Helper()
	databaseURL := os.Getenv("WIKI_TEST_DATABASE_URL")
	if databaseURL == "" {
		t.Skip("set WIKI_TEST_DATABASE_URL to a disposable local *_test database")
	}
	adminConfig, err := pgxpool.ParseConfig(databaseURL)
	if err != nil {
		t.Fatalf("parse WIKI_TEST_DATABASE_URL: %v", err)
	}
	host := adminConfig.ConnConfig.Host
	if host != "localhost" && host != "127.0.0.1" && host != "::1" {
		t.Fatalf("refusing non-local test database host %q", host)
	}
	if !strings.HasSuffix(adminConfig.ConnConfig.Database, "_test") {
		t.Fatalf("refusing database %q: name must end in _test", adminConfig.ConnConfig.Database)
	}

	ctx := context.Background()
	adminPool, err := pgxpool.NewWithConfig(ctx, adminConfig)
	if err != nil {
		t.Fatalf("connect admin pool: %v", err)
	}
	t.Cleanup(adminPool.Close)

	schema := "canvas_test_" + strings.ReplaceAll(uuid.NewString(), "-", "")
	if _, err := adminPool.Exec(ctx, "CREATE SCHEMA "+schema); err != nil {
		t.Fatalf("create isolated schema: %v", err)
	}
	t.Cleanup(func() { _, _ = adminPool.Exec(context.Background(), "DROP SCHEMA "+schema+" CASCADE") })

	testConfig, err := pgxpool.ParseConfig(databaseURL)
	if err != nil {
		t.Fatalf("parse isolated pool config: %v", err)
	}
	testConfig.ConnConfig.RuntimeParams["search_path"] = schema
	pool, err := pgxpool.NewWithConfig(ctx, testConfig)
	if err != nil {
		t.Fatalf("connect isolated pool: %v", err)
	}
	t.Cleanup(pool.Close)

	// The role every scoped transaction switches to. The repo package's fixture creates it the same way: both exception codes
	// are needed because roles are cluster-wide and parallel packages may race to create it.
	if _, err := pool.Exec(ctx, `
		DO $role$
		BEGIN
		    CREATE ROLE dataplane_app NOLOGIN NOSUPERUSER NOBYPASSRLS;
		EXCEPTION WHEN duplicate_object OR unique_violation THEN
		    NULL;
		END
		$role$;
	`); err != nil {
		t.Fatalf("create the scoped runtime role: %v", err)
	}
	for _, file := range []string{canvasMigrationFile, canvasFenceMigrationFile} {
		applyCanvasMigration(ctx, t, pool, schema, file)
	}
	// Stand-ins for the wiki tables, so the no-wiki-row check has tables to count. The canvas code never reads or writes them.
	if _, err := pool.Exec(ctx, `
		CREATE TABLE wiki_pages (page_id TEXT PRIMARY KEY, org_id TEXT NOT NULL);
		CREATE TABLE wiki_event_outbox (event_id BIGSERIAL PRIMARY KEY, org_id TEXT NOT NULL);
	`); err != nil {
		t.Fatalf("create the wiki stand-in tables: %v", err)
	}
	if _, err := pool.Exec(ctx, "GRANT USAGE ON SCHEMA "+schema+" TO dataplane_app"); err != nil {
		t.Fatalf("grant schema usage to the scoped role: %v", err)
	}
	return pool
}

// applyCanvasMigration runs one migration file in the test schema. The file's public. qualifiers name the test schema, so its
// GRANT and policy statements act on this schema's tables.
func applyCanvasMigration(ctx context.Context, t *testing.T, pool *pgxpool.Pool, schema, file string) {
	t.Helper()
	path := filepath.Join("..", "..", "..", "..", "infra", "postgres", "migrations", file)
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read migration %s: %v", file, err)
	}
	if _, err := pool.Exec(ctx, strings.ReplaceAll(string(raw), "public.", schema+".")); err != nil {
		t.Fatalf("apply migration %s: %v", file, err)
	}
}

func canvasAuthor() Author {
	return Author{Kind: AuthorUser, ID: "user-2"}
}

func agentAuthor(runID string) Author {
	return Author{Kind: AuthorAgent, ID: "run:" + runID, OnBehalfOf: "user-2", RunID: runID}
}

func canvasProvenance(key string) Provenance {
	return Provenance{DecisionRef: "decision-" + key, AuthorityRevision: 7, AudienceRevision: 4}
}

// createCanvas makes a Space's canvas with one version and returns that write.
func createCanvas(ctx context.Context, t *testing.T, repo *Repo, org, space, content string) WriteResult {
	t.Helper()
	res, err := saveCanvas(ctx, repo, org, space, "", nil, content, "create-"+space, canvasAuthor())
	if err != nil {
		t.Fatalf("create the canvas of %s/%s: %v", org, space, err)
	}
	if !res.Inserted || res.Version.ChangeKind != changeCreate {
		t.Fatalf("create of %s/%s = %+v, want an inserted create", org, space, res)
	}
	return res
}

// saveCanvas writes content over the expected head. An empty canvasID with a nil expected head is a create.
func saveCanvas(ctx context.Context, repo *Repo, org, space, canvasID string, expected *string, content, key string, author Author) (WriteResult, error) {
	return repo.Save(ctx, SaveInput{
		OrgID:             org,
		SpaceRef:          space,
		CanvasID:          canvasID,
		ExpectedVersionID: expected,
		Content:           content,
		IdempotencyKey:    key,
		Author:            author,
		Provenance:        canvasProvenance(key),
	})
}

// restoreCanvas copies the text of one version of the canvas into a new head.
func restoreCanvas(ctx context.Context, repo *Repo, org, space, canvasID, from string, expected *string, key string) (WriteResult, error) {
	return repo.Restore(ctx, RestoreInput{
		OrgID:                org,
		SpaceRef:             space,
		CanvasID:             canvasID,
		RestoreFromVersionID: from,
		ExpectedVersionID:    expected,
		IdempotencyKey:       key,
		Author:               canvasAuthor(),
		Provenance:           canvasProvenance(key),
	})
}

// countRows counts rows with a query run outside any org scope, on the owner connection.
func countRows(ctx context.Context, t *testing.T, pool *pgxpool.Pool, query string, args ...any) int {
	t.Helper()
	var n int
	if err := pool.QueryRow(ctx, query, args...).Scan(&n); err != nil {
		t.Fatalf("count rows (%s): %v", query, err)
	}
	return n
}

// countScoped counts rows the way the scoped role of one org sees them.
func countScoped(ctx context.Context, t *testing.T, pool *pgxpool.Pool, org, query string) int {
	t.Helper()
	n, err := orgscope.InOrgScope(ctx, pool, org, func(ctx context.Context, tx pgx.Tx) (int, error) {
		var n int
		err := tx.QueryRow(ctx, query).Scan(&n)
		return n, err
	})
	if err != nil {
		t.Fatalf("count rows as %s (%s): %v", org, query, err)
	}
	return n
}

func versionsOfCanvas(ctx context.Context, t *testing.T, pool *pgxpool.Pool, canvasID string) int {
	t.Helper()
	return countRows(ctx, t, pool, `SELECT count(*) FROM space_canvas_versions WHERE canvas_id = $1`, canvasID)
}

func canvasesOfSpace(ctx context.Context, t *testing.T, pool *pgxpool.Pool, org, space string) int {
	t.Helper()
	return countRows(ctx, t, pool, `SELECT count(*) FROM space_canvases WHERE org_id = $1 AND space_ref = $2`, org, space)
}

func fencesOfSpace(ctx context.Context, t *testing.T, pool *pgxpool.Pool, org, space string) int {
	t.Helper()
	return countRows(ctx, t, pool, `SELECT count(*) FROM space_canvas_purges WHERE org_id = $1 AND space_ref = $2`, org, space)
}

// requireSQLState fails unless err is a Postgres error with the given SQLSTATE.
func requireSQLState(t *testing.T, what string, err error, state string) {
	t.Helper()
	var pgErr *pgconn.PgError
	if !errors.As(err, &pgErr) || pgErr.Code != state {
		t.Fatalf("%s: want SQLSTATE %s, got %v", what, state, err)
	}
}

// TestCanvasConcurrentSavesOnOneHeadHaveOneWinner races twenty saves against one head. One lands, and the other nineteen
// are refused as canvas_head_moved and store nothing.
func TestCanvasConcurrentSavesOnOneHeadHaveOneWinner(t *testing.T) {
	pool := setupCanvasPool(t)
	repo := NewRepo(pool)
	ctx := context.Background()
	base := createCanvas(ctx, t, repo, "org-a", "space-a", "base")
	head := base.Version.VersionID

	const writers = 20
	outcomes := make(chan canvasOutcome, writers)
	start := make(chan struct{})
	var wg sync.WaitGroup
	for i := 0; i < writers; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			expected := head
			<-start
			res, err := saveCanvas(ctx, repo, "org-a", "space-a", base.CanvasID, &expected,
				fmt.Sprintf("writer %d", i), fmt.Sprintf("race-save-%d", i), canvasAuthor())
			outcomes <- canvasOutcome{res: res, err: err}
		}(i)
	}
	close(start)
	wg.Wait()
	close(outcomes)

	winners, refused := 0, 0
	for out := range outcomes {
		switch {
		case out.err == nil && out.res.Inserted:
			winners++
		case out.err != nil:
			requireCanvasError(t, out.err, CodeHeadMoved)
			refused++
		default:
			t.Errorf("a save returned neither an error nor an insert: %+v", out.res)
		}
	}
	if winners != 1 || refused != writers-1 {
		t.Fatalf("winners = %d, refused = %d; want 1 and %d", winners, refused, writers-1)
	}
	if n := versionsOfCanvas(ctx, t, pool, base.CanvasID); n != 2 {
		t.Fatalf("the canvas holds %d versions, want the base and the one winner", n)
	}
}

// TestCanvasConcurrentCreatorsGiveOneCanvas races two creates for one Space. One creates the canvas and the other is refused
// as canvas_head_moved, so the Space ends with one canvas.
func TestCanvasConcurrentCreatorsGiveOneCanvas(t *testing.T) {
	pool := setupCanvasPool(t)
	repo := NewRepo(pool)
	ctx := context.Background()

	const creators = 2
	outcomes := make(chan canvasOutcome, creators)
	start := make(chan struct{})
	var wg sync.WaitGroup
	for i := 0; i < creators; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			<-start
			res, err := saveCanvas(ctx, repo, "org-a", "space-a", "", nil,
				fmt.Sprintf("creator %d", i), fmt.Sprintf("create-race-%d", i), canvasAuthor())
			outcomes <- canvasOutcome{res: res, err: err}
		}(i)
	}
	close(start)
	wg.Wait()
	close(outcomes)

	created, refused := 0, 0
	for out := range outcomes {
		switch {
		case out.err == nil && out.res.Inserted && out.res.Version.ChangeKind == changeCreate:
			created++
		case out.err != nil:
			requireCanvasError(t, out.err, CodeHeadMoved)
			refused++
		default:
			t.Errorf("a create returned neither an error nor a created version: %+v", out.res)
		}
	}
	if created != 1 || refused != creators-1 {
		t.Fatalf("created = %d, refused = %d; want 1 and %d", created, refused, creators-1)
	}
	if n := canvasesOfSpace(ctx, t, pool, "org-a", "space-a"); n != 1 {
		t.Fatalf("the Space has %d canvases, want one", n)
	}
}

// TestCanvasSameKeyReplayIsDeduplicatedAndChangedContentConflicts replays a save under its own key, which stores nothing new,
// and then sends different content under the same key, which is a conflict.
func TestCanvasSameKeyReplayIsDeduplicatedAndChangedContentConflicts(t *testing.T) {
	pool := setupCanvasPool(t)
	repo := NewRepo(pool)
	ctx := context.Background()
	base := createCanvas(ctx, t, repo, "org-a", "space-a", "base")
	v1 := base.Version.VersionID

	first, err := saveCanvas(ctx, repo, "org-a", "space-a", base.CanvasID, ptr(v1), "edit", "same-key", canvasAuthor())
	if err != nil || !first.Inserted {
		t.Fatalf("first save = %+v, %v; want an inserted version", first, err)
	}

	replay, err := saveCanvas(ctx, repo, "org-a", "space-a", base.CanvasID, ptr(v1), "edit", "same-key", canvasAuthor())
	if err != nil || !replay.Deduplicated || replay.Inserted || replay.Version.VersionID != first.Version.VersionID {
		t.Fatalf("replay = %+v, %v; want the first version, deduplicated", replay, err)
	}

	_, err = saveCanvas(ctx, repo, "org-a", "space-a", base.CanvasID, ptr(v1), "a different edit", "same-key", canvasAuthor())
	requireCanvasError(t, err, CodeIdempotencyKeyConflict)

	if n := versionsOfCanvas(ctx, t, pool, base.CanvasID); n != 2 {
		t.Fatalf("the canvas holds %d versions after the replay and the conflict, want 2", n)
	}
}

// TestCanvasRestoreRecordsItsSourceAndIsUnchangedForMatchingText restores an earlier version and checks the stamped change
// kind and source. Restoring text the head already holds writes nothing.
func TestCanvasRestoreRecordsItsSourceAndIsUnchangedForMatchingText(t *testing.T) {
	pool := setupCanvasPool(t)
	repo := NewRepo(pool)
	ctx := context.Background()
	base := createCanvas(ctx, t, repo, "org-a", "space-a", "alpha")
	v1 := base.Version.VersionID

	edited, err := saveCanvas(ctx, repo, "org-a", "space-a", base.CanvasID, ptr(v1), "beta", "edit-beta", canvasAuthor())
	if err != nil {
		t.Fatalf("edit: %v", err)
	}
	v2 := edited.Version.VersionID

	restored, err := restoreCanvas(ctx, repo, "org-a", "space-a", base.CanvasID, v1, ptr(v2), "restore-alpha")
	if err != nil || !restored.Inserted {
		t.Fatalf("restore = %+v, %v; want an inserted version", restored, err)
	}
	if restored.Version.ChangeKind != changeRestore || restored.Version.VersionNo != 3 ||
		restored.Version.RestoredFromVersionID == nil || *restored.Version.RestoredFromVersionID != v1 {
		t.Fatalf("restored version = %+v; want change_kind restore, version 3, restored from %s", restored.Version, v1)
	}

	again, err := restoreCanvas(ctx, repo, "org-a", "space-a", base.CanvasID, v1, ptr(restored.Version.VersionID), "restore-again")
	if err != nil || !again.Unchanged || again.Inserted {
		t.Fatalf("restore of matching text = %+v, %v; want unchanged", again, err)
	}
	if n := versionsOfCanvas(ctx, t, pool, base.CanvasID); n != 3 {
		t.Fatalf("the canvas holds %d versions, want 3 (the unchanged restore stores nothing)", n)
	}
}

// TestCanvasIdsFromAnotherOrgOrSpaceAreNotFound presents version and canvas ids that belong to another org or Space. Each one
// is not found, and the refused writes store nothing.
func TestCanvasIdsFromAnotherOrgOrSpaceAreNotFound(t *testing.T) {
	pool := setupCanvasPool(t)
	repo := NewRepo(pool)
	ctx := context.Background()
	mine := createCanvas(ctx, t, repo, "org-a", "space-a", "alpha")
	other := createCanvas(ctx, t, repo, "org-b", "space-b", "bravo")

	_, err := repo.GetVersion(ctx, "org-a", "space-a", other.Version.VersionID, Floor{})
	requireCanvasError(t, err, CodeVersionNotFound)
	_, err = repo.GetVersion(ctx, "org-a", "space-b", other.Version.VersionID, Floor{})
	requireCanvasError(t, err, CodeVersionNotFound)
	_, err = repo.GetVersion(ctx, "org-b", "space-a", mine.Version.VersionID, Floor{})
	requireCanvasError(t, err, CodeVersionNotFound)
	_, err = repo.Head(ctx, "org-b", "space-a", nil)
	requireCanvasError(t, err, CodeCanvasNotFound)

	// A restore may copy only from a version of the canvas it names.
	_, err = restoreCanvas(ctx, repo, "org-a", "space-a", mine.CanvasID, other.Version.VersionID, ptr(mine.Version.VersionID), "cross-canvas-restore")
	requireCanvasError(t, err, CodeVersionNotFound)
	// A restore that names another org's canvas, in an org with no canvas for the Space, finds none.
	_, err = restoreCanvas(ctx, repo, "org-c", "space-c", mine.CanvasID, mine.Version.VersionID, nil, "guessed-canvas-restore")
	requireCanvasError(t, err, CodeCanvasNotFound)

	if n := versionsOfCanvas(ctx, t, pool, mine.CanvasID); n != 1 {
		t.Fatalf("org-a's canvas holds %d versions, want 1", n)
	}
	if n := versionsOfCanvas(ctx, t, pool, other.CanvasID); n != 1 {
		t.Fatalf("org-b's canvas holds %d versions, want 1", n)
	}
}

// TestCanvasRoleCannotChangeOrDeleteAVersionOrWriteAFence checks the privilege the migrations grant. The scoped role may read
// and append a version, and it may not update or delete one. It may read the Space fence and may not write it.
func TestCanvasRoleCannotChangeOrDeleteAVersionOrWriteAFence(t *testing.T) {
	pool := setupCanvasPool(t)
	repo := NewRepo(pool)
	ctx := context.Background()
	c := createCanvas(ctx, t, repo, "org-a", "space-a", "alpha")
	versionID := c.Version.VersionID

	cases := []struct{ what, query string }{
		{"update a version", fmt.Sprintf("UPDATE space_canvas_versions SET summary = 'rewritten' WHERE version_id = '%s'", versionID)},
		{"delete a version", fmt.Sprintf("DELETE FROM space_canvas_versions WHERE version_id = '%s'", versionID)},
		{"insert a Space fence", "INSERT INTO space_canvas_purges (org_id, space_ref) VALUES ('org-a', 'space-b')"},
		{"delete a Space fence", "DELETE FROM space_canvas_purges WHERE org_id = 'org-a'"},
	}
	for _, tc := range cases {
		_, err := orgscope.InOrgScope(ctx, pool, "org-a", func(ctx context.Context, tx pgx.Tx) (struct{}, error) {
			_, err := tx.Exec(ctx, tc.query)
			return struct{}{}, err
		})
		requireSQLState(t, "the scoped role may not "+tc.what, err, "42501")
	}

	if n := countRows(ctx, t, pool, `SELECT count(*) FROM space_canvas_versions WHERE version_id = $1 AND summary IS NULL`, versionID); n != 1 {
		t.Fatalf("the version changed or vanished under the scoped role (%d matching rows)", n)
	}
	if n := countScoped(ctx, t, pool, "org-a", `SELECT count(*) FROM space_canvas_purges`); n != 0 {
		t.Fatalf("the scoped role reads %d fences for org-a, want 0 (none written)", n)
	}
}

// TestCanvasRowsAreIsolatedByOrg checks row-level security. Each org's scope sees its own canvases and versions, and another
// org's version id is not found even when it is named directly.
func TestCanvasRowsAreIsolatedByOrg(t *testing.T) {
	pool := setupCanvasPool(t)
	repo := NewRepo(pool)
	ctx := context.Background()
	createCanvas(ctx, t, repo, "org-a", "space-a", "alpha")
	createCanvas(ctx, t, repo, "org-a", "space-a2", "alpha two")
	other := createCanvas(ctx, t, repo, "org-b", "space-b", "bravo")

	if n := countScoped(ctx, t, pool, "org-a", `SELECT count(*) FROM space_canvases`); n != 2 {
		t.Fatalf("org-a sees %d canvases, want its 2", n)
	}
	if n := countScoped(ctx, t, pool, "org-a", `SELECT count(*) FROM space_canvas_versions`); n != 2 {
		t.Fatalf("org-a sees %d versions, want its 2", n)
	}
	if n := countScoped(ctx, t, pool, "org-b", `SELECT count(*) FROM space_canvases`); n != 1 {
		t.Fatalf("org-b sees %d canvases, want its 1", n)
	}
	if n := countScoped(ctx, t, pool, "org-b", `SELECT count(*) FROM space_canvas_versions`); n != 1 {
		t.Fatalf("org-b sees %d versions, want its 1", n)
	}
	_, err := repo.GetVersion(ctx, "org-a", "space-b", other.Version.VersionID, Floor{})
	requireCanvasError(t, err, CodeVersionNotFound)
}

// TestCanvasCompositeForeignKeyRefusesAVersionOfAnotherOrg inserts a version that names org-b with org-a's canvas. The
// composite key refuses it.
func TestCanvasCompositeForeignKeyRefusesAVersionOfAnotherOrg(t *testing.T) {
	pool := setupCanvasPool(t)
	repo := NewRepo(pool)
	ctx := context.Background()
	c := createCanvas(ctx, t, repo, "org-a", "space-a", "alpha")

	_, err := pool.Exec(ctx, `
		INSERT INTO space_canvas_versions (version_id, canvas_id, org_id, version_no, content, content_sha256, content_bytes,
			author_kind, author_id, change_kind, decision_ref, authority_revision, audience_revision, idempotency_hash)
		VALUES ('forged-version', $1, 'org-b', 2, 'forged', $2, 6, 'user', 'user-2', 'edit', 'decision-forged', 1, 1, 'forged-hash')`,
		c.CanvasID, ContentDigest("forged"))
	requireSQLState(t, "a version naming org-b with org-a's canvas", err, "23503")

	if n := versionsOfCanvas(ctx, t, pool, c.CanvasID); n != 1 {
		t.Fatalf("the refused version was stored: the canvas holds %d versions, want 1", n)
	}
}

// TestCanvasAuthorWriteLimitIsSixtyWritesPerHour counts the create as one of the author's writes. The sixty-first write is
// refused as canvas_rate_limited.
func TestCanvasAuthorWriteLimitIsSixtyWritesPerHour(t *testing.T) {
	pool := setupCanvasPool(t)
	repo := NewRepo(pool)
	ctx := context.Background()
	base := createCanvas(ctx, t, repo, "org-a", "space-a", "draft 1")
	head := base.Version.VersionID

	for i := 2; i <= MaxWritesPerAuthorPerHour; i++ {
		res, err := saveCanvas(ctx, repo, "org-a", "space-a", base.CanvasID, ptr(head),
			fmt.Sprintf("draft %d", i), fmt.Sprintf("draft-%d", i), canvasAuthor())
		if err != nil {
			t.Fatalf("write %d of %d refused: %v", i, MaxWritesPerAuthorPerHour, err)
		}
		head = res.Version.VersionID
	}

	_, err := saveCanvas(ctx, repo, "org-a", "space-a", base.CanvasID, ptr(head), "one write too many", "draft-over", canvasAuthor())
	requireCanvasError(t, err, CodeRateLimited)
	if n := versionsOfCanvas(ctx, t, pool, base.CanvasID); n != MaxWritesPerAuthorPerHour {
		t.Fatalf("the canvas holds %d versions, want %d", n, MaxWritesPerAuthorPerHour)
	}
}

// TestCanvasAgentRunWriteLimitIsTwentyWritesPerRun refuses an agent run's twenty-first write as canvas_run_write_limit. A
// different run keeps its own allowance.
func TestCanvasAgentRunWriteLimitIsTwentyWritesPerRun(t *testing.T) {
	pool := setupCanvasPool(t)
	repo := NewRepo(pool)
	ctx := context.Background()
	base := createCanvas(ctx, t, repo, "org-a", "space-a", "alpha")
	head := base.Version.VersionID

	run := agentAuthor("run-limit-1")
	for i := 1; i <= MaxWritesPerAgentRun; i++ {
		res, err := saveCanvas(ctx, repo, "org-a", "space-a", base.CanvasID, ptr(head),
			fmt.Sprintf("agent draft %d", i), fmt.Sprintf("agent-%d", i), run)
		if err != nil {
			t.Fatalf("agent write %d of %d refused: %v", i, MaxWritesPerAgentRun, err)
		}
		head = res.Version.VersionID
	}

	_, err := saveCanvas(ctx, repo, "org-a", "space-a", base.CanvasID, ptr(head), "agent write over the limit", "agent-over", run)
	requireCanvasError(t, err, CodeRunWriteLimit)

	if _, err := saveCanvas(ctx, repo, "org-a", "space-a", base.CanvasID, ptr(head), "a second run", "agent-other-run", agentAuthor("run-limit-2")); err != nil {
		t.Fatalf("a second run was refused: %v", err)
	}
}

// TestCanvasPurgeSpaceRemovesRowsAndAReplayReturnsZero purges one Space. Its canvas and versions go, the other org's canvas
// stays, and a replay finds nothing.
func TestCanvasPurgeSpaceRemovesRowsAndAReplayReturnsZero(t *testing.T) {
	pool := setupCanvasPool(t)
	repo := NewRepo(pool)
	ctx := context.Background()
	kept := createCanvas(ctx, t, repo, "org-b", "space-b", "bravo")
	purged := createCanvas(ctx, t, repo, "org-a", "space-a", "alpha")
	if _, err := saveCanvas(ctx, repo, "org-a", "space-a", purged.CanvasID, ptr(purged.Version.VersionID), "alpha two", "alpha-two", canvasAuthor()); err != nil {
		t.Fatalf("second version: %v", err)
	}

	counts, err := repo.PurgeSpace(ctx, "org-a", "space-a")
	if err != nil || counts.CanvasesPurged != 1 || counts.VersionsPurged != 2 {
		t.Fatalf("purge = %+v, %v; want 1 canvas and 2 versions", counts, err)
	}
	if n := canvasesOfSpace(ctx, t, pool, "org-a", "space-a"); n != 0 {
		t.Fatalf("the purged Space keeps %d canvases", n)
	}
	if n := versionsOfCanvas(ctx, t, pool, purged.CanvasID); n != 0 {
		t.Fatalf("the purged canvas keeps %d versions", n)
	}
	if n := versionsOfCanvas(ctx, t, pool, kept.CanvasID); n != 1 {
		t.Fatalf("another org's canvas holds %d versions, want 1", n)
	}

	again, err := repo.PurgeSpace(ctx, "org-a", "space-a")
	if err != nil || again != (PurgeCounts{}) {
		t.Fatalf("replayed purge = %+v, %v; want zero counts", again, err)
	}
}

// TestCanvasWritesNeverTouchTheWikiTables runs every canvas call and then counts the wiki rows. None may appear, since room text
// stays out of the org-wide wiki publication stream.
func TestCanvasWritesNeverTouchTheWikiTables(t *testing.T) {
	pool := setupCanvasPool(t)
	repo := NewRepo(pool)
	ctx := context.Background()
	c := createCanvas(ctx, t, repo, "org-a", "space-a", "alpha")
	edited, err := saveCanvas(ctx, repo, "org-a", "space-a", c.CanvasID, ptr(c.Version.VersionID), "beta", "edit-beta", canvasAuthor())
	if err != nil {
		t.Fatalf("save: %v", err)
	}
	if _, err := restoreCanvas(ctx, repo, "org-a", "space-a", c.CanvasID, c.Version.VersionID, ptr(edited.Version.VersionID), "restore-alpha"); err != nil {
		t.Fatalf("restore: %v", err)
	}
	if _, err := repo.PurgeSpace(ctx, "org-a", "space-a"); err != nil {
		t.Fatalf("purge: %v", err)
	}
	for _, table := range []string{"wiki_pages", "wiki_event_outbox"} {
		if n := countRows(ctx, t, pool, "SELECT count(*) FROM "+table); n != 0 {
			t.Fatalf("%s holds %d rows after canvas writes", table, n)
		}
	}
}

// TestCanvasWriteForAPurgedCanvasIsRefusedAndStoresNothing sends a save, a create that names a head and a restore decided for a
// purged canvas. The Space holds no canvas, so each is canvas_not_found (D21n): no head moved, the canvas is gone.
func TestCanvasWriteForAPurgedCanvasIsRefusedAndStoresNothing(t *testing.T) {
	pool := setupCanvasPool(t)
	repo := NewRepo(pool)
	ctx := context.Background()
	c := createCanvas(ctx, t, repo, "org-a", "space-a", "alpha")
	v1 := c.Version.VersionID
	if _, err := repo.PurgeSpace(ctx, "org-a", "space-a"); err != nil {
		t.Fatalf("purge: %v", err)
	}

	_, err := saveCanvas(ctx, repo, "org-a", "space-a", c.CanvasID, ptr(v1), "late save", "late-save", canvasAuthor())
	requireCanvasError(t, err, CodeCanvasNotFound)
	_, err = saveCanvas(ctx, repo, "org-a", "space-a", "", ptr(v1), "late create", "late-create", canvasAuthor())
	requireCanvasError(t, err, CodeCanvasNotFound)
	_, err = restoreCanvas(ctx, repo, "org-a", "space-a", c.CanvasID, v1, ptr(v1), "late-restore")
	requireCanvasError(t, err, CodeCanvasNotFound)

	if n := versionsOfCanvas(ctx, t, pool, c.CanvasID); n != 0 {
		t.Fatalf("refused writes stored %d versions for the purged canvas", n)
	}
	if n := canvasesOfSpace(ctx, t, pool, "org-a", "space-a"); n != 0 {
		t.Fatalf("refused writes left %d canvases in the Space", n)
	}
}

// TestCanvasWriteForAReplacedCanvasIsHeadMoved replaces a canvas by erasing its org and creating a new one. A save and a
// restore decided for the old canvas are refused as canvas_head_moved and store nothing.
func TestCanvasWriteForAReplacedCanvasIsHeadMoved(t *testing.T) {
	pool := setupCanvasPool(t)
	repo := NewRepo(pool)
	ctx := context.Background()
	old := createCanvas(ctx, t, repo, "org-a", "space-a", "alpha")

	// Org erasure removes the canvas and the Space's fence, so the Space can take a new canvas afterwards.
	tx, err := pool.Begin(ctx)
	if err != nil {
		t.Fatalf("begin org purge: %v", err)
	}
	if _, err := PurgeOrgTx(ctx, tx, "org-a"); err != nil {
		_ = tx.Rollback(ctx)
		t.Fatalf("org purge: %v", err)
	}
	if err := tx.Commit(ctx); err != nil {
		t.Fatalf("commit org purge: %v", err)
	}
	replacement := createCanvas(ctx, t, repo, "org-a", "space-a", "beta")
	if replacement.CanvasID == old.CanvasID {
		t.Fatal("the replacement canvas reused the purged canvas id")
	}

	_, err = saveCanvas(ctx, repo, "org-a", "space-a", old.CanvasID, ptr(old.Version.VersionID), "stale save", "stale-save", canvasAuthor())
	requireCanvasError(t, err, CodeHeadMoved)
	_, err = restoreCanvas(ctx, repo, "org-a", "space-a", old.CanvasID, old.Version.VersionID, ptr(old.Version.VersionID), "stale-restore")
	requireCanvasError(t, err, CodeHeadMoved)

	if n := versionsOfCanvas(ctx, t, pool, replacement.CanvasID); n != 1 {
		t.Fatalf("the replacement canvas holds %d versions, want only its create", n)
	}
}

// TestCanvasRestoreWithNoCanvasIsCanvasNotFound restores into a Space that has never had a canvas.
func TestCanvasRestoreWithNoCanvasIsCanvasNotFound(t *testing.T) {
	pool := setupCanvasPool(t)
	repo := NewRepo(pool)
	ctx := context.Background()

	_, err := restoreCanvas(ctx, repo, "org-a", "space-a", "canvas-never-created", "version-never-created", nil, "restore-empty")
	requireCanvasError(t, err, CodeCanvasNotFound)
	if n := canvasesOfSpace(ctx, t, pool, "org-a", "space-a"); n != 0 {
		t.Fatalf("a restore created %d canvases", n)
	}
}

// TestCanvasCreateIsRefusedWhenACanvasExistsOrTheSpaceIsFenced refuses a second create while the canvas exists, and a create
// after the Space is purged, which the fence answers as canvas_not_found.
func TestCanvasCreateIsRefusedWhenACanvasExistsOrTheSpaceIsFenced(t *testing.T) {
	pool := setupCanvasPool(t)
	repo := NewRepo(pool)
	ctx := context.Background()
	createCanvas(ctx, t, repo, "org-a", "space-a", "alpha")

	_, err := saveCanvas(ctx, repo, "org-a", "space-a", "", nil, "second create", "second-create", canvasAuthor())
	requireCanvasError(t, err, CodeHeadMoved)

	if _, err := repo.PurgeSpace(ctx, "org-a", "space-a"); err != nil {
		t.Fatalf("purge: %v", err)
	}
	_, err = saveCanvas(ctx, repo, "org-a", "space-a", "", nil, "create after purge", "create-after-purge", canvasAuthor())
	requireCanvasError(t, err, CodeCanvasNotFound)

	if n := canvasesOfSpace(ctx, t, pool, "org-a", "space-a"); n != 0 {
		t.Fatalf("the fenced Space holds %d canvases", n)
	}
}

// TestCanvasPurgeWritesTheFenceWhenTheSpaceHasNoCanvas purges a Space that never had a canvas. The fence is written, a replay
// returns zero counts and leaves one fence, and a create for the Space is refused.
func TestCanvasPurgeWritesTheFenceWhenTheSpaceHasNoCanvas(t *testing.T) {
	pool := setupCanvasPool(t)
	repo := NewRepo(pool)
	ctx := context.Background()

	counts, err := repo.PurgeSpace(ctx, "org-a", "space-empty")
	if err != nil || counts != (PurgeCounts{}) {
		t.Fatalf("purge of an empty Space = %+v, %v; want zero counts", counts, err)
	}
	if n := fencesOfSpace(ctx, t, pool, "org-a", "space-empty"); n != 1 {
		t.Fatalf("the purge wrote %d fences, want 1", n)
	}

	again, err := repo.PurgeSpace(ctx, "org-a", "space-empty")
	if err != nil || again != (PurgeCounts{}) {
		t.Fatalf("replayed purge = %+v, %v; want zero counts", again, err)
	}
	if n := fencesOfSpace(ctx, t, pool, "org-a", "space-empty"); n != 1 {
		t.Fatalf("a replayed purge left %d fences, want 1", n)
	}

	_, err = saveCanvas(ctx, repo, "org-a", "space-empty", "", nil, "late create", "late-create", canvasAuthor())
	requireCanvasError(t, err, CodeCanvasNotFound)
}

// TestCanvasTwentyConcurrentCreatesAndAPurgeLeaveNoCanvas races twenty creates against one purge of the same Space. Whichever
// order they run in, the Space ends with no canvas and one fence.
func TestCanvasTwentyConcurrentCreatesAndAPurgeLeaveNoCanvas(t *testing.T) {
	pool := setupCanvasPool(t)
	repo := NewRepo(pool)
	ctx := context.Background()

	const creators = 20
	outcomes := make(chan canvasOutcome, creators)
	purged := make(chan error, 1)
	start := make(chan struct{})
	var wg sync.WaitGroup
	for i := 0; i < creators; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			<-start
			res, err := saveCanvas(ctx, repo, "org-a", "space-a", "", nil,
				fmt.Sprintf("racing create %d", i), fmt.Sprintf("racing-create-%d", i), canvasAuthor())
			outcomes <- canvasOutcome{res: res, err: err}
		}(i)
	}
	wg.Add(1)
	go func() {
		defer wg.Done()
		<-start
		_, err := repo.PurgeSpace(ctx, "org-a", "space-a")
		purged <- err
	}()
	close(start)
	wg.Wait()
	close(outcomes)
	close(purged)

	for out := range outcomes {
		if out.err == nil {
			continue
		}
		var typed *Error
		if !errors.As(out.err, &typed) || (typed.Code != CodeHeadMoved && typed.Code != CodeCanvasNotFound) {
			t.Fatalf("a create racing the purge failed with %v, want canvas_head_moved or canvas_not_found", out.err)
		}
	}
	if err := <-purged; err != nil {
		t.Fatalf("purge: %v", err)
	}
	if n := canvasesOfSpace(ctx, t, pool, "org-a", "space-a"); n != 0 {
		t.Fatalf("the Space keeps %d canvases after the purge", n)
	}
	if n := countRows(ctx, t, pool, `SELECT count(*) FROM space_canvas_versions WHERE org_id = 'org-a'`); n != 0 {
		t.Fatalf("org-a keeps %d versions after the purge", n)
	}
	if n := fencesOfSpace(ctx, t, pool, "org-a", "space-a"); n != 1 {
		t.Fatalf("the Space has %d fences, want 1", n)
	}
}
