package skillbundle

import (
	"context"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

// retentionTx records every statement of a retained bundle write and answers the few reads the
// store makes. accepted is the skill's review state; live is its live bundle's digest ("" none).
type retentionTx struct {
	pgx.Tx
	accepted  bool
	live      string
	execs     []string
	committed bool
}

func (tx *retentionTx) Exec(_ context.Context, sql string, _ ...any) (pgconn.CommandTag, error) {
	tx.execs = append(tx.execs, sql)
	return pgconn.NewCommandTag("UPDATE 1"), nil
}

func (tx *retentionTx) QueryRow(_ context.Context, sql string, _ ...any) pgx.Row {
	switch {
	case strings.Contains(sql, "review_state = 'accepted'"):
		return retentionRow{value: tx.accepted}
	case strings.Contains(sql, "SELECT manifest_sha256 FROM skill_bundles"):
		if tx.live == "" {
			return retentionRow{err: pgx.ErrNoRows}
		}
		return retentionRow{value: tx.live}
	case strings.Contains(sql, "UPDATE skill_bundles SET state = CASE"):
		tx.execs = append(tx.execs, sql)
		return retentionRow{err: pgx.ErrNoRows}
	case strings.Contains(sql, "INSERT INTO skill_bundles"):
		tx.execs = append(tx.execs, sql)
		return retentionRow{value: time.Date(2026, 11, 5, 9, 0, 0, 0, time.UTC)}
	}
	return retentionRow{err: pgx.ErrNoRows}
}

func (tx *retentionTx) Commit(context.Context) error {
	tx.committed = true
	return nil
}

func (tx *retentionTx) Rollback(context.Context) error { return nil }

type retentionRow struct {
	value any
	err   error
}

func (row retentionRow) Scan(dest ...any) error {
	if row.err != nil {
		return row.err
	}
	switch typed := dest[0].(type) {
	case *bool:
		*typed = row.value.(bool)
	case *string:
		*typed = row.value.(string)
	case *time.Time:
		*typed = row.value.(time.Time)
	}
	return nil
}

type retentionDB struct{ tx *retentionTx }

func (d retentionDB) Begin(context.Context) (pgx.Tx, error) { return d.tx, nil }
func (d retentionDB) Exec(context.Context, string, ...any) (pgconn.CommandTag, error) {
	return pgconn.CommandTag{}, nil
}
func (d retentionDB) Query(context.Context, string, ...any) (pgx.Rows, error) { return nil, nil }
func (d retentionDB) QueryRow(context.Context, string, ...any) pgx.Row       { return nil }

func retentionBundle() Bundle {
	return Bundle{
		ManifestSHA256: strings.Repeat("b", 64),
		TotalBytes:     5,
		Files:          []File{{Path: "SKILL.md", SHA256: strings.Repeat("c", 64), Size: 5, Body: []byte("hello")}},
	}
}

func executed(execs []string, fragment string) bool {
	for _, sql := range execs {
		if strings.Contains(sql, fragment) {
			return true
		}
	}
	return false
}

func TestRetentionFromLookupIsStrict(t *testing.T) {
	for value, want := range map[string]bool{"true": true, "": false, "1": false, "TRUE": false, "yes": false} {
		got := RetentionFromLookup(func(name string) string {
			if name == RetentionEnv {
				return value
			}
			return ""
		})
		if got != want {
			t.Errorf("RetentionFromLookup(%q) = %v, want %v", value, got, want)
		}
	}
}

// With retention on, a new upload keeps the old live bundle as superseded instead of deleting it,
// so a run pinned to the version that named it can still read it.
func TestBundleSupersededNotDeleted(t *testing.T) {
	tx := &retentionTx{accepted: true, live: strings.Repeat("a", 64)}
	result, err := NewStore(retentionDB{tx: tx}).WithRetention(true).Put(context.Background(), "org-a", "skill-1", "user-a", retentionBundle())
	if err != nil {
		t.Fatalf("Put: %v", err)
	}
	if !tx.committed || !result.Replaced || result.Draft {
		t.Fatalf("result = %+v, committed = %v", result, tx.committed)
	}
	for _, sql := range tx.execs {
		if strings.Contains(sql, "DELETE FROM skill_bundles") && !strings.Contains(sql, "state") {
			t.Fatalf("a retained upload must not delete the skill's bundles: %s", sql)
		}
	}
	if !executed(tx.execs, "SET state = 'superseded'") {
		t.Fatal("the replaced live bundle must become superseded")
	}
	if !executed(tx.execs, retainedGCSQL) {
		t.Fatal("a retained upload must still collect superseded bundles no version names")
	}
}

// With drafts on, an upload to an accepted skill is its draft's bundle: the live bundle stays and
// no version is cut until the draft is published.
func TestDraftUploadLeavesTheLiveBundle(t *testing.T) {
	tx := &retentionTx{accepted: true, live: strings.Repeat("a", 64)}
	store := NewStore(retentionDB{tx: tx}).WithRetention(true).WithDrafts(true).WithSkillVersions(true)
	result, err := store.Put(context.Background(), "org-a", "skill-1", "user-a", retentionBundle())
	if err != nil {
		t.Fatalf("Put: %v", err)
	}
	if !result.Draft {
		t.Fatal("an upload to an accepted skill with drafts on must be a draft")
	}
	if executed(tx.execs, "SET state = 'superseded'") {
		t.Fatal("a draft upload must leave the live bundle live")
	}
	if executed(tx.execs, stampSkillBundleSQL) {
		t.Fatal("a draft upload must not stamp the live bundle digest (that cuts a version)")
	}
	if !executed(tx.execs, stampDraftBundleSQL) {
		t.Fatal("a draft upload must name its bundle in the skill's draft")
	}
}

// A proposal has no draft: accepting it is its publish, so its bundle stays live as before.
func TestDraftsLeaveAProposalsBundleLive(t *testing.T) {
	tx := &retentionTx{accepted: false}
	result, err := NewStore(retentionDB{tx: tx}).WithRetention(true).WithDrafts(true).Put(context.Background(), "org-a", "skill-1", "user-a", retentionBundle())
	if err != nil {
		t.Fatalf("Put: %v", err)
	}
	if result.Draft || executed(tx.execs, stampDraftBundleSQL) {
		t.Fatal("a proposal's upload must not open a draft")
	}
}

// Drafts without retention change nothing: the store runs today's statements.
func TestDraftsNeedRetention(t *testing.T) {
	store := NewStore(nil).WithDrafts(true)
	if store.retention {
		t.Fatal("WithDrafts must not turn retention on")
	}
}

// The collection keeps a superseded bundle while any version of its skill, or the skill's draft,
// names its manifest digest, and never touches a live or draft bundle.
func TestRetainedGCKeepsBundlesNamedByVersions(t *testing.T) {
	for _, fragment := range []string{
		"sb.state = 'superseded'",
		"FROM agent_skill_versions v",
		"v.skill_id = sb.skill_id AND v.bundle_sha256 = sb.manifest_sha256",
		"s.draft->>'bundle_sha256' = sb.manifest_sha256",
	} {
		if !strings.Contains(retainedGCSQL, fragment) {
			t.Errorf("retainedGCSQL lacks %q", fragment)
		}
	}
}

// Deleting a skill removes every bundle it has, in any state, and the blobs only they named.
func TestSkillDeleteRemovesBundles(t *testing.T) {
	tx := &retentionTx{}
	if err := DeleteSkillBundles(context.Background(), tx, "org-a", "skill-1"); err != nil {
		t.Fatalf("DeleteSkillBundles: %v", err)
	}
	if !executed(tx.execs, "DELETE FROM skill_bundles WHERE org_id = $1 AND skill_id = $2") {
		t.Fatal("the skill's bundles must be deleted whatever their state")
	}
	if executed(tx.execs, "state") {
		t.Fatal("a skill delete must not filter its bundles by state")
	}
	if !executed(tx.execs, gcBlobsSQL) {
		t.Fatal("the blobs only the skill's bundles named must go")
	}
}

// Publishing a draft that names a bundle the skill no longer holds is refused, so the skill is
// never pointed at files that are not there.
func TestPublishDraftBundleRefusesAMissingBundle(t *testing.T) {
	tx := &missingBundleTx{retentionTx: retentionTx{live: strings.Repeat("a", 64)}}
	err := PublishDraftBundle(context.Background(), tx, "org-a", "skill-1", "user-a", strings.Repeat("d", 64))
	if err != ErrDraftBundleMissing {
		t.Fatalf("PublishDraftBundle = %v, want ErrDraftBundleMissing", err)
	}
}

// missingBundleTx answers the publish flip with no row.
type missingBundleTx struct{ retentionTx }

func (tx *missingBundleTx) Exec(ctx context.Context, sql string, args ...any) (pgconn.CommandTag, error) {
	if strings.Contains(sql, "SET state = 'live'") {
		return pgconn.NewCommandTag("UPDATE 0"), nil
	}
	return tx.retentionTx.Exec(ctx, sql, args...)
}
