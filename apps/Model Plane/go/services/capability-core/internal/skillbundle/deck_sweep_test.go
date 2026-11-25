package skillbundle

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

// F-f09-08: the deck template stamp and the startup sweep. A scripted fake answers the statements the sweep makes from
// in-memory bundles, so the sweep's own logic runs without a database; the statements themselves are checked against a
// real Postgres by deck_validated_integration_test.go.

// sweepBundle is one stored live bundle the fake answers for.
type sweepBundle struct {
	id, orgID, skillID string
	bundle             Bundle
	stamped            bool
}

// sweepDB answers the sweep's statements: the candidate list, the manifest read by bundle id, the blob read by digest,
// and the stamp, which marks the live bundle with the digest it names. phantoms are candidate rows with no bundle behind
// them, so a read of one finds nothing.
type sweepDB struct {
	bundles  []*sweepBundle
	phantoms int
	bound    []any
	stamps   []string
	lastSQL  string
}

func (d *sweepDB) Begin(context.Context) (pgx.Tx, error) {
	return sweepTx{db: d}, nil
}

// sweepTx is the transaction a stamp runs in: its statements go to the same fake, and it commits at once.
type sweepTx struct {
	pgx.Tx
	db *sweepDB
}

func (tx sweepTx) Exec(ctx context.Context, sql string, args ...any) (pgconn.CommandTag, error) {
	return tx.db.Exec(ctx, sql, args...)
}

func (tx sweepTx) Commit(context.Context) error   { return nil }
func (tx sweepTx) Rollback(context.Context) error { return nil }

func (d *sweepDB) Exec(_ context.Context, sql string, args ...any) (pgconn.CommandTag, error) {
	d.lastSQL = sql
	if strings.Contains(sql, "pg_advisory_xact_lock") {
		return pgconn.NewCommandTag("SELECT 1"), nil
	}
	if !strings.Contains(sql, "SET deck_validated_at") {
		return pgconn.CommandTag{}, errors.New("unexpected exec in the sweep fake")
	}
	orgID, skillID, manifest := args[0].(string), args[1].(string), args[2].(string)
	for _, stored := range d.bundles {
		if stored.orgID == orgID && stored.skillID == skillID && stored.bundle.ManifestSHA256 == manifest && !stored.stamped {
			stored.stamped = true
			d.stamps = append(d.stamps, skillID)
			return pgconn.NewCommandTag("UPDATE 1"), nil
		}
	}
	return pgconn.NewCommandTag("UPDATE 0"), nil
}

func (d *sweepDB) Query(_ context.Context, sql string, args ...any) (pgx.Rows, error) {
	d.lastSQL = sql
	switch {
	case strings.Contains(sql, "deck_validated_at IS NULL"):
		d.bound = args
		rows := [][]any{}
		for _, stored := range d.bundles {
			if stored.stamped || !hasSweepFile(stored.bundle, ThemePath) {
				continue
			}
			rows = append(rows, []any{stored.id, stored.orgID, stored.skillID, stored.bundle.ManifestSHA256})
		}
		for i := 0; i < d.phantoms; i++ {
			rows = append(rows, []any{fmt.Sprintf("skb_ghost_%d", i), "org-a", "skill-ghost", "digest-ghost"})
		}
		return &sweepRows{rows: rows}, nil
	case strings.Contains(sql, "WHERE b.org_id = $1 AND ($2::text[] IS NULL"):
		// The list statement: no bundle in this fake, so no summary; the test reads the statement itself.
		return &sweepRows{}, nil
	case strings.Contains(sql, "WHERE b.id = $1"):
		rows := [][]any{}
		for _, stored := range d.bundles {
			if stored.id != args[0].(string) {
				continue
			}
			for _, file := range stored.bundle.Files {
				rows = append(rows, []any{stored.bundle.ManifestSHA256, file.Path, file.SHA256, file.Size, file.Executable, file.Binary})
			}
		}
		return &sweepRows{rows: rows}, nil
	}
	return nil, errors.New("unexpected query in the sweep fake")
}

func (d *sweepDB) QueryRow(_ context.Context, sql string, args ...any) pgx.Row {
	d.lastSQL = sql
	if !strings.Contains(sql, "SELECT b.content") {
		return sweepValue{err: errors.New("unexpected query row in the sweep fake")}
	}
	orgID, skillID, digest := args[0].(string), args[1].(string), args[2].(string)
	for _, stored := range d.bundles {
		if stored.orgID != orgID || stored.skillID != skillID {
			continue
		}
		for _, file := range stored.bundle.Files {
			if file.SHA256 == digest {
				return sweepValue{value: file.Body}
			}
		}
	}
	return sweepValue{err: pgx.ErrNoRows}
}

func hasSweepFile(bundle Bundle, path string) bool {
	for _, file := range bundle.Files {
		if file.Path == path {
			return true
		}
	}
	return false
}

// sweepRows is the rows of one fake statement.
type sweepRows struct {
	rows [][]any
	next int
}

func (r *sweepRows) Close()                                       {}
func (r *sweepRows) Err() error                                   { return nil }
func (r *sweepRows) CommandTag() pgconn.CommandTag                { return pgconn.CommandTag{} }
func (r *sweepRows) FieldDescriptions() []pgconn.FieldDescription { return nil }
func (r *sweepRows) RawValues() [][]byte                          { return nil }
func (r *sweepRows) Conn() *pgx.Conn                              { return nil }
func (r *sweepRows) Values() ([]any, error)                       { return r.rows[r.next-1], nil }

func (r *sweepRows) Next() bool {
	if r.next >= len(r.rows) {
		return false
	}
	r.next++
	return true
}

func (r *sweepRows) Scan(dest ...any) error {
	row := r.rows[r.next-1]
	for i, target := range dest {
		if err := assignSweepValue(target, row[i]); err != nil {
			return err
		}
	}
	return nil
}

// sweepValue is one fake row for QueryRow.
type sweepValue struct {
	value any
	err   error
}

func (v sweepValue) Scan(dest ...any) error {
	if v.err != nil {
		return v.err
	}
	return assignSweepValue(dest[0], v.value)
}

// assignSweepValue copies one fake column into the scan target by the target's type.
func assignSweepValue(target, value any) error {
	switch typed := target.(type) {
	case *string:
		*typed = value.(string)
	case *int64:
		*typed = value.(int64)
	case *bool:
		*typed = value.(bool)
	case *[]byte:
		*typed = value.([]byte)
	case *time.Time:
		*typed = value.(time.Time)
	default:
		return fmt.Errorf("sweep fake cannot scan into %T", target)
	}
	return nil
}

// storedDeckBundle is a validated deck template stored as a live bundle of the fake.
func storedDeckBundle(id, skillID string, bundle Bundle) *sweepBundle {
	return &sweepBundle{id: id, orgID: "org-a", skillID: skillID, bundle: bundle}
}

func TestDeckSweepStampsACleanBundleAndLeavesAFailingOneUnstamped(t *testing.T) {
	clean := templateBundle(t, themeFixture(t))
	scripted := templateBundle(t, themeFixture(t), zipEntry{name: "templates/helper", body: []byte("#!/bin/sh\necho hi\n")})
	db := &sweepDB{bundles: []*sweepBundle{
		storedDeckBundle("skb_clean", "skill-clean", clean),
		storedDeckBundle("skb_scripted", "skill-scripted", scripted),
	}}
	checked, passed, failed, err := NewStore(db).RevalidateDeckTemplates(context.Background())
	if err != nil || checked != 2 || passed != 1 || failed != 1 {
		t.Fatalf("sweep = (%d, %d, %d, %v), want (2, 1, 1, nil)", checked, passed, failed, err)
	}
	if !db.bundles[0].stamped || db.bundles[1].stamped {
		t.Fatalf("stamped clean=%v scripted=%v, want true false", db.bundles[0].stamped, db.bundles[1].stamped)
	}
	if len(db.stamps) != 1 || db.stamps[0] != "skill-clean" {
		t.Fatalf("stamps = %v, want only skill-clean", db.stamps)
	}
	// The failing bundle is kept exactly as it was: its files are still there and nothing was deleted.
	if len(db.bundles[1].bundle.Files) != len(scripted.Files) {
		t.Fatalf("the failing bundle changed: %d files, want %d", len(db.bundles[1].bundle.Files), len(scripted.Files))
	}
}

func TestDeckSweepIsIdempotentAndDoesNotRecheckAStampedBundle(t *testing.T) {
	db := &sweepDB{bundles: []*sweepBundle{storedDeckBundle("skb_clean", "skill-clean", templateBundle(t, themeFixture(t)))}}
	store := NewStore(db)
	if _, passed, _, err := store.RevalidateDeckTemplates(context.Background()); err != nil || passed != 1 {
		t.Fatalf("first sweep: passed=%d err=%v, want 1 nil", passed, err)
	}
	checked, passed, failed, err := store.RevalidateDeckTemplates(context.Background())
	if err != nil || checked != 0 || passed != 0 || failed != 0 {
		t.Fatalf("second sweep = (%d, %d, %d, %v), want nothing to check", checked, passed, failed, err)
	}
	if len(db.stamps) != 1 {
		t.Fatalf("stamps after two sweeps = %v, want one", db.stamps)
	}
}

func TestDeckSweepChecksAtMostFiveHundredBundlesOldestFirst(t *testing.T) {
	db := &sweepDB{phantoms: deckSweepLimit + 100}
	checked, passed, failed, err := NewStore(db).RevalidateDeckTemplates(context.Background())
	if err != nil || checked != deckSweepLimit || passed != 0 || failed != deckSweepLimit {
		t.Fatalf("sweep = (%d, %d, %d, %v), want (%d, 0, %d, nil)", checked, passed, failed, err, deckSweepLimit, deckSweepLimit)
	}
	if len(db.bound) != 2 || db.bound[0] != ThemePath || db.bound[1] != deckSweepLimit {
		t.Fatalf("candidate query args = %v, want the theme path and the bound %d", db.bound, deckSweepLimit)
	}
}

func TestDeckSweepStopsWhenItsContextEnds(t *testing.T) {
	db := &sweepDB{bundles: []*sweepBundle{storedDeckBundle("skb_clean", "skill-clean", templateBundle(t, themeFixture(t)))}}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	checked, _, _, err := NewStore(db).RevalidateDeckTemplates(ctx)
	if !errors.Is(err, context.Canceled) || checked != 0 || len(db.stamps) != 0 {
		t.Fatalf("cancelled sweep = (checked %d, stamps %v, %v), want no work and context.Canceled", checked, db.stamps, err)
	}
}

func TestMarkDeckValidatedIgnoresAStaleDigest(t *testing.T) {
	db := &sweepDB{bundles: []*sweepBundle{storedDeckBundle("skb_new", "skill-a", templateBundle(t, themeFixture(t)))}}
	store := NewStore(db)
	if err := store.MarkDeckValidated(context.Background(), "org-a", "skill-a", "replaced-digest"); err != nil {
		t.Fatalf("a stale digest is not an error: %v", err)
	}
	if db.bundles[0].stamped {
		t.Fatal("a stale digest stamped the bundle that replaced it")
	}
	if err := store.MarkDeckValidated(context.Background(), "org-a", "skill-a", db.bundles[0].bundle.ManifestSHA256); err != nil {
		t.Fatalf("MarkDeckValidated: %v", err)
	}
	if !db.bundles[0].stamped {
		t.Fatal("the current digest was not stamped")
	}
}

func TestDeckTemplateListingNarrowsToStampedBundles(t *testing.T) {
	db := &sweepDB{}
	if _, err := NewStore(db).List(context.Background(), "org-a", nil); err != nil {
		t.Fatalf("List: %v", err)
	}
	if !strings.Contains(db.lastSQL, "AND b.deck_validated_at IS NOT NULL") {
		t.Fatalf("the list statement does not require the deck stamp:\n%s", db.lastSQL)
	}
}
