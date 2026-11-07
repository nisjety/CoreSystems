package skillbundle

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

// ErrNotFound means the org holds no bundle for the skill, or no file of it has that digest.
var ErrNotFound = errors.New("skill bundle not found")

// DB is the subset of the capability-core pool the store uses. Writes run in a transaction,
// so a replaced bundle is never left half-written.
type DB interface {
	Begin(ctx context.Context) (pgx.Tx, error)
	Exec(ctx context.Context, sql string, args ...any) (pgconn.CommandTag, error)
	Query(ctx context.Context, sql string, args ...any) (pgx.Rows, error)
	QueryRow(ctx context.Context, sql string, args ...any) pgx.Row
}

// Store reads and writes skill bundles. Every statement filters on the org, and a blob is
// addressed by org and digest, so one org never reads another org's bytes.
type Store struct {
	db DB
	// skillVersions is SKILL_VERSIONS_API (EP-11 S2): on, Put and Delete also write the skill's
	// bundle_sha256 in the same transaction, so a bundle change is a new skill version.
	skillVersions bool
	// retention is SKILL_BUNDLE_RETENTION (EP-11 S5): on, a replaced bundle is kept as
	// superseded while a skill version names it, instead of being deleted. Off, Put and Delete
	// run exactly the statements they ran before.
	retention bool
	// drafts is SKILL_DRAFT_PUBLISH (EP-11 S5), honoured only with retention on: an upload to an
	// accepted skill becomes its draft's bundle, live only once the draft is published.
	drafts bool
}

// RetentionEnv is the switch for keeping skill bundles per version (EP-11 S5). Only the literal
// "true" turns it on. It must stay off until bundle read authorization (register D2-379, D2-383)
// is fixed, because retained bundles stay readable longer.
const RetentionEnv = "SKILL_BUNDLE_RETENTION"

// RetentionFromLookup reads SKILL_BUNDLE_RETENTION strictly.
func RetentionFromLookup(lookup func(string) string) bool {
	return lookup(RetentionEnv) == "true"
}

// NewStore wraps the pool.
func NewStore(db DB) *Store {
	return &Store{db: db}
}

// WithSkillVersions turns on the bundle digest stamp on agent_skills (EP-11 S2). Off, the store
// writes only its own tables, as before.
func (s *Store) WithSkillVersions(on bool) *Store {
	s.skillVersions = on
	return s
}

// WithRetention turns on per-version bundle retention (EP-11 S5).
func (s *Store) WithRetention(on bool) *Store {
	s.retention = on
	return s
}

// WithDrafts makes an upload to an accepted skill a draft bundle (EP-11 S5). It has no effect
// while retention is off: without retention a draft bundle could not outlive the next upload.
func (s *Store) WithDrafts(on bool) *Store {
	s.drafts = on
	return s
}

// stampSkillActorSQL names the uploader as the actor of the version the stamp cuts, with the
// transaction-local settings session-core's skill triggers read (migration 0064 and 0108). An
// upload or delete is always a person's (skillBundleWriter).
const stampSkillActorSQL = `SELECT set_config('mp.actor_kind', 'user', true), set_config('mp.actor_id', $1, true)`

// stampSkillBundleSQL writes the skill's bundle digest ('' when it has none). The column is
// versioned, so session-core's trigger cuts a new version of an accepted skill whose digest moved.
const stampSkillBundleSQL = `UPDATE agent_skills SET bundle_sha256 = $3 WHERE org_id = $1 AND id = $2 AND bundle_sha256 <> $3`

// stampSkill records manifest as the skill's bundle digest inside tx, when the stamp is on.
func (s *Store) stampSkill(ctx context.Context, tx pgx.Tx, orgID, skillID, actor, manifest string) error {
	if !s.skillVersions {
		return nil
	}
	if _, err := tx.Exec(ctx, stampSkillActorSQL, actor); err != nil {
		return fmt.Errorf("name skill bundle actor: %w", err)
	}
	if _, err := tx.Exec(ctx, stampSkillBundleSQL, orgID, skillID, manifest); err != nil {
		return fmt.Errorf("stamp skill bundle digest: %w", err)
	}
	return nil
}

// Summary describes one stored bundle without its files.
type Summary struct {
	SkillID        string
	ManifestSHA256 string
	FileCount      int
	TotalBytes     int64
	CreatedBy      string
	CreatedAt      time.Time
	// DeckTemplate is true when the bundle holds ThemePath (EP-5 S6). Derived from the bundle's
	// files on every read rather than stored, so no migration carries it and it cannot go stale.
	DeckTemplate bool
}

// Manifest is one stored bundle's files, in path order, with bodies left out.
type Manifest struct {
	SkillID        string
	ManifestSHA256 string
	Files          []File
}

// PutResult says whether the upload replaced a bundle the skill already had.
type PutResult struct {
	Replaced bool
	Summary  Summary
	// Draft is true when the upload became the skill's draft bundle (EP-11 S5): the live bundle
	// is unchanged until the draft is published. Always false while retention is off.
	Draft bool
}

// lockOrgSQL serializes bundle writes for one org. The blob GC in a write must not race a
// concurrent write that is about to reference the same digest, and the lock is released with
// the transaction.
const lockOrgSQL = `SELECT pg_advisory_xact_lock(hashtextextended('skill_bundles:' || $1::text, 0))`

// Put stores a validated bundle for the skill, replacing any bundle it had. The old bundle's
// files go with it through the cascade, and any blob no bundle of the org still names is
// dropped. The write is one transaction that also appends a registry_audit_log row which
// names the digest and the totals, never a path or a body. With retention on, putRetained keeps
// the replaced bundle instead.
func (s *Store) Put(ctx context.Context, orgID, skillID, actor string, bundle Bundle) (PutResult, error) {
	if s.retention {
		return s.putRetained(ctx, orgID, skillID, actor, bundle)
	}
	tx, err := s.db.Begin(ctx)
	if err != nil {
		return PutResult{}, fmt.Errorf("begin skill bundle write: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	if _, err := tx.Exec(ctx, lockOrgSQL, orgID); err != nil {
		return PutResult{}, fmt.Errorf("lock skill bundles: %w", err)
	}
	deleted, err := tx.Exec(ctx, `DELETE FROM skill_bundles WHERE org_id = $1 AND skill_id = $2`, orgID, skillID)
	if err != nil {
		return PutResult{}, fmt.Errorf("replace skill bundle: %w", err)
	}
	bundleID := "skb_" + uuid.NewString()
	var createdAt time.Time
	err = tx.QueryRow(ctx, `
		INSERT INTO skill_bundles (id, org_id, skill_id, manifest_sha256, file_count, total_bytes, created_by)
		VALUES ($1, $2, $3, $4, $5, $6, $7)
		RETURNING created_at
	`, bundleID, orgID, skillID, bundle.ManifestSHA256, len(bundle.Files), bundle.TotalBytes, actor).Scan(&createdAt)
	if err != nil {
		return PutResult{}, fmt.Errorf("insert skill bundle: %w", err)
	}
	if err := storeFiles(ctx, tx, orgID, bundleID, bundle); err != nil {
		return PutResult{}, err
	}
	if _, err := tx.Exec(ctx, gcBlobsSQL, orgID); err != nil {
		return PutResult{}, fmt.Errorf("collect skill bundle blobs: %w", err)
	}
	if err := appendAudit(ctx, tx, "skill_bundle.uploaded", orgID, skillID, actor, bundle.ManifestSHA256,
		len(bundle.Files), bundle.TotalBytes); err != nil {
		return PutResult{}, err
	}
	if err := s.stampSkill(ctx, tx, orgID, skillID, actor, bundle.ManifestSHA256); err != nil {
		return PutResult{}, err
	}
	if err := tx.Commit(ctx); err != nil {
		return PutResult{}, fmt.Errorf("commit skill bundle write: %w", err)
	}
	return PutResult{
		Replaced: deleted.RowsAffected() > 0,
		Summary: Summary{
			SkillID: skillID, ManifestSHA256: bundle.ManifestSHA256, FileCount: len(bundle.Files),
			TotalBytes: bundle.TotalBytes, CreatedBy: actor, CreatedAt: createdAt,
		},
	}, nil
}

// storeFiles writes a new bundle row's blobs (once per org and digest) and file rows.
func storeFiles(ctx context.Context, tx pgx.Tx, orgID, bundleID string, bundle Bundle) error {
	blobDigests, blobBodies, blobSizes := make([]string, 0, len(bundle.Files)), make([][]byte, 0, len(bundle.Files)), make([]int32, 0, len(bundle.Files))
	seen := make(map[string]bool, len(bundle.Files))
	paths, digests, sizes, executables := make([]string, 0, len(bundle.Files)), make([]string, 0, len(bundle.Files)), make([]int32, 0, len(bundle.Files)), make([]bool, 0, len(bundle.Files))
	binaries := make([]bool, 0, len(bundle.Files))
	for _, file := range bundle.Files {
		if !seen[file.SHA256] {
			seen[file.SHA256] = true
			blobDigests = append(blobDigests, file.SHA256)
			blobBodies = append(blobBodies, file.Body)
			blobSizes = append(blobSizes, int32(file.Size))
		}
		paths = append(paths, file.Path)
		digests = append(digests, file.SHA256)
		sizes = append(sizes, int32(file.Size))
		executables = append(executables, file.Executable)
		binaries = append(binaries, file.Binary)
	}
	if _, err := tx.Exec(ctx, `
		INSERT INTO skill_bundle_blobs (org_id, sha256, content, size_bytes)
		SELECT $1, u.sha256, u.content, u.size_bytes
		FROM unnest($2::text[], $3::bytea[], $4::int[]) AS u(sha256, content, size_bytes)
		ON CONFLICT (org_id, sha256) DO NOTHING
	`, orgID, blobDigests, blobBodies, blobSizes); err != nil {
		return fmt.Errorf("store skill bundle blobs: %w", err)
	}
	if _, err := tx.Exec(ctx, `
		INSERT INTO skill_bundle_files (bundle_id, path, sha256, size_bytes, executable, is_binary)
		SELECT $1, u.path, u.sha256, u.size_bytes, u.executable, u.is_binary
		FROM unnest($2::text[], $3::text[], $4::int[], $5::bool[], $6::bool[])
		     AS u(path, sha256, size_bytes, executable, is_binary)
	`, bundleID, paths, digests, sizes, executables, binaries); err != nil {
		return fmt.Errorf("store skill bundle files: %w", err)
	}
	return nil
}

// acceptedSkillSQL says whether the skill is an accepted one, whose text and bundle change only
// through a draft while drafts are on. A proposal's bundle stays live: accepting it is its publish.
const acceptedSkillSQL = `SELECT EXISTS (SELECT 1 FROM agent_skills WHERE org_id = $1 AND id = $2 AND review_state = 'accepted')`

// stampDraftBundleSQL records manifest as the bundle of the skill's draft, opening the draft from
// the live fields when there is none (the body function is session-core's, migration 0108/0109).
// The live fields are untouched, so no version is cut until the draft is published.
const stampDraftBundleSQL = `
	UPDATE agent_skills
	SET draft = COALESCE(draft, agent_skill_version_body(agent_skills)) || jsonb_build_object('bundle_sha256', $3::text),
	    draft_updated_at = now(), draft_by = $4
	WHERE org_id = $1 AND id = $2`

// retainedGCSQL drops the org's superseded bundles that neither a version of their skill nor the
// skill's draft names. Their files cascade; gcBlobsSQL then drops the blobs no bundle names.
const retainedGCSQL = `
DELETE FROM skill_bundles sb
WHERE sb.org_id = $1 AND sb.state = 'superseded'
  AND NOT EXISTS (
      SELECT 1 FROM agent_skill_versions v
      WHERE v.org_id = sb.org_id AND v.skill_id = sb.skill_id AND v.bundle_sha256 = sb.manifest_sha256
  )
  AND NOT EXISTS (
      SELECT 1 FROM agent_skills s
      WHERE s.org_id = sb.org_id AND s.id = sb.skill_id AND s.draft->>'bundle_sha256' = sb.manifest_sha256
  )
`

// collectRetained runs both collections, superseded bundles first so their blobs go with them.
func collectRetained(ctx context.Context, tx Execer, orgID string) error {
	if _, err := tx.Exec(ctx, retainedGCSQL, orgID); err != nil {
		return fmt.Errorf("collect superseded skill bundles: %w", err)
	}
	if _, err := tx.Exec(ctx, gcBlobsSQL, orgID); err != nil {
		return fmt.Errorf("collect skill bundle blobs: %w", err)
	}
	return nil
}

// Execer is the statement side of a transaction; the registry's skill transaction and a pgx.Tx
// both satisfy it.
type Execer interface {
	Exec(ctx context.Context, sql string, args ...any) (pgconn.CommandTag, error)
	QueryRow(ctx context.Context, sql string, args ...any) pgx.Row
}

// stampDraft records manifest as the draft's bundle, naming the uploader as the actor the
// revision trigger reads (opening a draft is an `edited` revision).
func stampDraft(ctx context.Context, tx Execer, orgID, skillID, actor, manifest string) error {
	if _, err := tx.Exec(ctx, stampSkillActorSQL, actor); err != nil {
		return fmt.Errorf("name skill bundle actor: %w", err)
	}
	if _, err := tx.Exec(ctx, stampDraftBundleSQL, orgID, skillID, manifest, actor); err != nil {
		return fmt.Errorf("stamp skill draft bundle: %w", err)
	}
	return nil
}

// draftMode says whether a bundle write to the skill goes to its draft.
func (s *Store) draftMode(ctx context.Context, tx Execer, orgID, skillID string) (bool, error) {
	if !s.drafts {
		return false, nil
	}
	var accepted bool
	if err := tx.QueryRow(ctx, acceptedSkillSQL, orgID, skillID).Scan(&accepted); err != nil {
		return false, fmt.Errorf("read skill review state: %w", err)
	}
	return accepted, nil
}

// liveManifest is the digest of the skill's live bundle, "" when it has none.
func liveManifest(ctx context.Context, tx Execer, orgID, skillID string) (string, error) {
	var manifest string
	err := tx.QueryRow(ctx, `
		SELECT manifest_sha256 FROM skill_bundles WHERE org_id = $1 AND skill_id = $2 AND state = 'live'
	`, orgID, skillID).Scan(&manifest)
	if errors.Is(err, pgx.ErrNoRows) {
		return "", nil
	}
	if err != nil {
		return "", fmt.Errorf("read live skill bundle: %w", err)
	}
	return manifest, nil
}

// putRetained is Put with retention on (EP-11 S5). The replaced live bundle becomes superseded
// and stays while a version of the skill names it; an upload of bytes the skill already holds
// takes that row back instead of storing them twice. With drafts on, an upload to an accepted
// skill is its draft's bundle and the live bundle stays until the draft is published.
func (s *Store) putRetained(ctx context.Context, orgID, skillID, actor string, bundle Bundle) (PutResult, error) {
	tx, err := s.db.Begin(ctx)
	if err != nil {
		return PutResult{}, fmt.Errorf("begin skill bundle write: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	if _, err := tx.Exec(ctx, lockOrgSQL, orgID); err != nil {
		return PutResult{}, fmt.Errorf("lock skill bundles: %w", err)
	}
	draft, err := s.draftMode(ctx, tx, orgID, skillID)
	if err != nil {
		return PutResult{}, err
	}
	live, err := liveManifest(ctx, tx, orgID, skillID)
	if err != nil {
		return PutResult{}, err
	}
	target := "live"
	if draft {
		target = "draft"
		// One draft bundle per skill: an earlier draft upload of other bytes goes.
		if _, err := tx.Exec(ctx, `
			DELETE FROM skill_bundles WHERE org_id = $1 AND skill_id = $2 AND state = 'draft' AND manifest_sha256 <> $3
		`, orgID, skillID, bundle.ManifestSHA256); err != nil {
			return PutResult{}, fmt.Errorf("replace skill draft bundle: %w", err)
		}
	} else if live != bundle.ManifestSHA256 {
		if _, err := tx.Exec(ctx, `
			UPDATE skill_bundles SET state = 'superseded' WHERE org_id = $1 AND skill_id = $2 AND state = 'live'
		`, orgID, skillID); err != nil {
			return PutResult{}, fmt.Errorf("supersede skill bundle: %w", err)
		}
	}
	// A row with these bytes already: a live upload takes it back into live. A draft leaves its
	// state alone: a live or superseded row a draft names is kept by the draft (retainedGCSQL), and
	// discarding the draft must not delete a bundle an older version still names.
	var createdAt time.Time
	err = tx.QueryRow(ctx, `
		UPDATE skill_bundles SET state = CASE WHEN $4 = 'draft' THEN state ELSE 'live' END
		WHERE org_id = $1 AND skill_id = $2 AND manifest_sha256 = $3
		RETURNING created_at
	`, orgID, skillID, bundle.ManifestSHA256, target).Scan(&createdAt)
	if errors.Is(err, pgx.ErrNoRows) {
		bundleID := "skb_" + uuid.NewString()
		err = tx.QueryRow(ctx, `
			INSERT INTO skill_bundles (id, org_id, skill_id, manifest_sha256, file_count, total_bytes, created_by, state)
			VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
			RETURNING created_at
		`, bundleID, orgID, skillID, bundle.ManifestSHA256, len(bundle.Files), bundle.TotalBytes, actor, target).Scan(&createdAt)
		if err != nil {
			return PutResult{}, fmt.Errorf("insert skill bundle: %w", err)
		}
		if err := storeFiles(ctx, tx, orgID, bundleID, bundle); err != nil {
			return PutResult{}, err
		}
	} else if err != nil {
		return PutResult{}, fmt.Errorf("reuse skill bundle: %w", err)
	}
	action := "skill_bundle.uploaded"
	if draft {
		action = "skill_bundle.drafted"
		err = stampDraft(ctx, tx, orgID, skillID, actor, bundle.ManifestSHA256)
	} else {
		err = s.stampSkill(ctx, tx, orgID, skillID, actor, bundle.ManifestSHA256)
	}
	if err != nil {
		return PutResult{}, err
	}
	// After the stamp, so the version it cut already names the new bundle.
	if err := collectRetained(ctx, tx, orgID); err != nil {
		return PutResult{}, err
	}
	if err := appendAudit(ctx, tx, action, orgID, skillID, actor, bundle.ManifestSHA256,
		len(bundle.Files), bundle.TotalBytes); err != nil {
		return PutResult{}, err
	}
	if err := tx.Commit(ctx); err != nil {
		return PutResult{}, fmt.Errorf("commit skill bundle write: %w", err)
	}
	return PutResult{
		Replaced: live != "",
		Draft:    draft,
		Summary: Summary{
			SkillID: skillID, ManifestSHA256: bundle.ManifestSHA256, FileCount: len(bundle.Files),
			TotalBytes: bundle.TotalBytes, CreatedBy: actor, CreatedAt: createdAt,
		},
	}, nil
}

// Delete removes the org's bundle for the skill. Its files cascade, and blobs no bundle still
// names are dropped. ErrNotFound when there is no bundle to remove. With retention on,
// deleteRetained keeps the removed bundle while a version names it.
func (s *Store) Delete(ctx context.Context, orgID, skillID, actor string) error {
	if s.retention {
		return s.deleteRetained(ctx, orgID, skillID, actor)
	}
	tx, err := s.db.Begin(ctx)
	if err != nil {
		return fmt.Errorf("begin skill bundle delete: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	if _, err := tx.Exec(ctx, lockOrgSQL, orgID); err != nil {
		return fmt.Errorf("lock skill bundles: %w", err)
	}
	var manifest string
	var fileCount int
	var totalBytes int64
	err = tx.QueryRow(ctx, `
		DELETE FROM skill_bundles WHERE org_id = $1 AND skill_id = $2
		RETURNING manifest_sha256, file_count, total_bytes
	`, orgID, skillID).Scan(&manifest, &fileCount, &totalBytes)
	if errors.Is(err, pgx.ErrNoRows) {
		return ErrNotFound
	}
	if err != nil {
		return fmt.Errorf("delete skill bundle: %w", err)
	}
	if _, err := tx.Exec(ctx, gcBlobsSQL, orgID); err != nil {
		return fmt.Errorf("collect skill bundle blobs: %w", err)
	}
	if err := appendAudit(ctx, tx, "skill_bundle.deleted", orgID, skillID, actor, manifest, fileCount, totalBytes); err != nil {
		return err
	}
	if err := s.stampSkill(ctx, tx, orgID, skillID, actor, ""); err != nil {
		return err
	}
	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("commit skill bundle delete: %w", err)
	}
	return nil
}

// deleteRetained is Delete with retention on (EP-11 S5). The live bundle becomes superseded, so a
// run pinned to a version that named it still reads it. With drafts on, removing an accepted
// skill's bundle is a draft change (the draft names no bundle), live once published.
func (s *Store) deleteRetained(ctx context.Context, orgID, skillID, actor string) error {
	tx, err := s.db.Begin(ctx)
	if err != nil {
		return fmt.Errorf("begin skill bundle delete: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	if _, err := tx.Exec(ctx, lockOrgSQL, orgID); err != nil {
		return fmt.Errorf("lock skill bundles: %w", err)
	}
	draft, err := s.draftMode(ctx, tx, orgID, skillID)
	if err != nil {
		return err
	}
	var manifest string
	var fileCount int
	var totalBytes int64
	action := "skill_bundle.deleted"
	if draft {
		// The bundle the draft would publish: its own upload, else the live one.
		err = tx.QueryRow(ctx, `
			SELECT manifest_sha256, file_count, total_bytes FROM skill_bundles
			WHERE org_id = $1 AND skill_id = $2 AND state IN ('draft', 'live')
			ORDER BY state = 'draft' DESC LIMIT 1
		`, orgID, skillID).Scan(&manifest, &fileCount, &totalBytes)
		if errors.Is(err, pgx.ErrNoRows) {
			return ErrNotFound
		}
		if err != nil {
			return fmt.Errorf("read skill bundle: %w", err)
		}
		if _, err := tx.Exec(ctx, `
			DELETE FROM skill_bundles WHERE org_id = $1 AND skill_id = $2 AND state = 'draft'
		`, orgID, skillID); err != nil {
			return fmt.Errorf("delete skill draft bundle: %w", err)
		}
		action = "skill_bundle.draft_deleted"
		err = stampDraft(ctx, tx, orgID, skillID, actor, "")
	} else {
		err = tx.QueryRow(ctx, `
			UPDATE skill_bundles SET state = 'superseded' WHERE org_id = $1 AND skill_id = $2 AND state = 'live'
			RETURNING manifest_sha256, file_count, total_bytes
		`, orgID, skillID).Scan(&manifest, &fileCount, &totalBytes)
		if errors.Is(err, pgx.ErrNoRows) {
			return ErrNotFound
		}
		if err != nil {
			return fmt.Errorf("supersede skill bundle: %w", err)
		}
		err = s.stampSkill(ctx, tx, orgID, skillID, actor, "")
	}
	if err != nil {
		return err
	}
	if err := collectRetained(ctx, tx, orgID); err != nil {
		return err
	}
	if err := appendAudit(ctx, tx, action, orgID, skillID, actor, manifest, fileCount, totalBytes); err != nil {
		return err
	}
	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("commit skill bundle delete: %w", err)
	}
	return nil
}

// ErrDraftBundleMissing means a draft names a bundle the skill no longer holds, so publishing it
// would point the skill at files that are not there.
var ErrDraftBundleMissing = errors.New("skill draft bundle missing")

// PublishDraftBundle makes manifest the skill's live bundle inside the publish transaction tx
// (EP-11 S5): the live bundle becomes superseded, the row with manifest becomes live ("" leaves
// the skill with no live bundle), and any other draft bundle goes. It runs after the publish
// statement, so the version that statement cut already names the new bundle when the collection
// runs. Call it only with retention on.
func PublishDraftBundle(ctx context.Context, tx Execer, orgID, skillID, actor, manifest string) error {
	if _, err := tx.Exec(ctx, lockOrgSQL, orgID); err != nil {
		return fmt.Errorf("lock skill bundles: %w", err)
	}
	live, err := liveManifest(ctx, tx, orgID, skillID)
	if err != nil {
		return err
	}
	if live != manifest {
		if _, err := tx.Exec(ctx, `
			UPDATE skill_bundles SET state = 'superseded' WHERE org_id = $1 AND skill_id = $2 AND state = 'live'
		`, orgID, skillID); err != nil {
			return fmt.Errorf("supersede skill bundle: %w", err)
		}
		if manifest != "" {
			result, err := tx.Exec(ctx, `
				UPDATE skill_bundles SET state = 'live'
				WHERE org_id = $1 AND skill_id = $2 AND manifest_sha256 = $3
			`, orgID, skillID, manifest)
			if err != nil {
				return fmt.Errorf("publish skill bundle: %w", err)
			}
			if result.RowsAffected() == 0 {
				return ErrDraftBundleMissing
			}
		}
	}
	if _, err := tx.Exec(ctx, `
		DELETE FROM skill_bundles WHERE org_id = $1 AND skill_id = $2 AND state = 'draft'
	`, orgID, skillID); err != nil {
		return fmt.Errorf("clear skill draft bundle: %w", err)
	}
	if err := collectRetained(ctx, tx, orgID); err != nil {
		return err
	}
	if live == manifest {
		return nil
	}
	return appendAudit(ctx, tx, "skill_bundle.published", orgID, skillID, actor, manifest, 0, 0)
}

// DiscardDraftBundle removes the skill's draft bundle inside the discard transaction tx, after the
// draft itself was cleared, and collects what nothing names any more. Call it only with retention on.
func DiscardDraftBundle(ctx context.Context, tx Execer, orgID, skillID string) error {
	if _, err := tx.Exec(ctx, lockOrgSQL, orgID); err != nil {
		return fmt.Errorf("lock skill bundles: %w", err)
	}
	if _, err := tx.Exec(ctx, `
		DELETE FROM skill_bundles WHERE org_id = $1 AND skill_id = $2 AND state = 'draft'
	`, orgID, skillID); err != nil {
		return fmt.Errorf("discard skill draft bundle: %w", err)
	}
	return collectRetained(ctx, tx, orgID)
}

// DeleteSkillBundles removes every bundle of a skill that is being deleted, in the skill delete's
// transaction tx (closes register D2-382 for new deletes), and collects the blobs nothing names
// any more. The skill's versions go with the skill (session-core 0108), so nothing can pin them.
func DeleteSkillBundles(ctx context.Context, tx Execer, orgID, skillID string) error {
	if _, err := tx.Exec(ctx, lockOrgSQL, orgID); err != nil {
		return fmt.Errorf("lock skill bundles: %w", err)
	}
	if _, err := tx.Exec(ctx, `DELETE FROM skill_bundles WHERE org_id = $1 AND skill_id = $2`, orgID, skillID); err != nil {
		return fmt.Errorf("delete skill bundles: %w", err)
	}
	if _, err := tx.Exec(ctx, gcBlobsSQL, orgID); err != nil {
		return fmt.Errorf("collect skill bundle blobs: %w", err)
	}
	return nil
}

// HeldManifest says whether the skill still holds a bundle with manifest (live or superseded), so
// a restore can name it in a draft. A draft bundle does not count: it is not a published one.
func HeldManifest(ctx context.Context, tx Execer, orgID, skillID, manifest string) (bool, error) {
	var held bool
	err := tx.QueryRow(ctx, `
		SELECT EXISTS (SELECT 1 FROM skill_bundles
		WHERE org_id = $1 AND skill_id = $2 AND manifest_sha256 = $3 AND state <> 'draft')
	`, orgID, skillID, manifest).Scan(&held)
	if err != nil {
		return false, fmt.Errorf("read skill bundle: %w", err)
	}
	return held, nil
}

// gcBlobsSQL drops the org's blobs that no bundle file names any more.
const gcBlobsSQL = `
DELETE FROM skill_bundle_blobs b
WHERE b.org_id = $1
  AND NOT EXISTS (
      SELECT 1 FROM skill_bundle_files f
      JOIN skill_bundles sb ON sb.id = f.bundle_id
      WHERE sb.org_id = b.org_id AND f.sha256 = b.sha256
  )
`

// appendAudit writes the registry_audit_log row for a bundle change. The diff holds the
// digest and the totals only: a skill bundle's paths and bodies are never audit content.
func appendAudit(ctx context.Context, tx Execer, action, orgID, skillID, actor, manifest string, fileCount int, totalBytes int64) error {
	_, err := tx.Exec(ctx, `
		INSERT INTO registry_audit_log (id, entity_kind, entity_id, action, actor, org_id, diff_json)
		VALUES ($1, 'skill_bundle', $2, $3, $4, $5,
		        jsonb_build_object('manifest_sha256', $6::text, 'file_count', $7::int, 'total_bytes', $8::bigint))
	`, "ral_"+uuid.NewString(), skillID, action, actor, orgID, manifest, fileCount, totalBytes)
	if err != nil {
		return fmt.Errorf("append skill bundle audit: %w", err)
	}
	return nil
}

// List returns the org's bundle summaries, narrowed to skillIDs when any are given.
func (s *Store) List(ctx context.Context, orgID string, skillIDs []string) ([]Summary, error) {
	var filter []string
	if len(skillIDs) > 0 {
		filter = skillIDs
	}
	rows, err := s.db.Query(ctx, `
		SELECT b.skill_id, b.manifest_sha256, b.file_count, b.total_bytes, b.created_by, b.created_at,
		       EXISTS (SELECT 1 FROM skill_bundle_files f WHERE f.bundle_id = b.id AND f.path = $3)
		FROM skill_bundles b
		WHERE b.org_id = $1 AND ($2::text[] IS NULL OR b.skill_id = ANY($2::text[])) AND b.state = 'live'
		ORDER BY b.skill_id
	`, orgID, filter, ThemePath)
	if err != nil {
		return nil, fmt.Errorf("list skill bundles: %w", err)
	}
	defer rows.Close()
	summaries := []Summary{}
	for rows.Next() {
		var summary Summary
		if err := rows.Scan(&summary.SkillID, &summary.ManifestSHA256, &summary.FileCount,
			&summary.TotalBytes, &summary.CreatedBy, &summary.CreatedAt, &summary.DeckTemplate); err != nil {
			return nil, fmt.Errorf("scan skill bundle: %w", err)
		}
		summaries = append(summaries, summary)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("list skill bundles: %w", err)
	}
	return summaries, nil
}

// Manifest returns the org's bundle for the skill: its digest and every file's metadata.
// ErrNotFound when the org has no bundle for the skill.
func (s *Store) Manifest(ctx context.Context, orgID, skillID string) (Manifest, error) {
	return s.readManifest(ctx, skillID, `
		SELECT b.manifest_sha256, f.path, f.sha256, f.size_bytes, f.executable, f.is_binary
		FROM skill_bundles b
		JOIN skill_bundle_files f ON f.bundle_id = b.id
		WHERE b.org_id = $1 AND b.skill_id = $2 AND b.state = 'live'
		ORDER BY f.path
	`, orgID, skillID)
}

// ManifestAt returns the skill's bundle whose manifest digest is manifestSHA256, live or kept for a
// version (EP-11 S5), so a run pinned to an older version reads that version's files. A draft
// bundle is never returned: it is not published. ErrNotFound when the skill holds no such bundle.
func (s *Store) ManifestAt(ctx context.Context, orgID, skillID, manifestSHA256 string) (Manifest, error) {
	return s.readManifest(ctx, skillID, `
		SELECT b.manifest_sha256, f.path, f.sha256, f.size_bytes, f.executable, f.is_binary
		FROM skill_bundles b
		JOIN skill_bundle_files f ON f.bundle_id = b.id
		WHERE b.org_id = $1 AND b.skill_id = $2 AND b.manifest_sha256 = $3 AND b.state <> 'draft'
		ORDER BY f.path
	`, orgID, skillID, manifestSHA256)
}

// readManifest runs one manifest query and gathers its file rows.
func (s *Store) readManifest(ctx context.Context, skillID, query string, args ...any) (Manifest, error) {
	rows, err := s.db.Query(ctx, query, args...)
	if err != nil {
		return Manifest{}, fmt.Errorf("read skill bundle manifest: %w", err)
	}
	defer rows.Close()
	manifest := Manifest{SkillID: skillID, Files: []File{}}
	for rows.Next() {
		var file File
		if err := rows.Scan(&manifest.ManifestSHA256, &file.Path, &file.SHA256, &file.Size,
			&file.Executable, &file.Binary); err != nil {
			return Manifest{}, fmt.Errorf("scan skill bundle file: %w", err)
		}
		manifest.Files = append(manifest.Files, file)
	}
	if err := rows.Err(); err != nil {
		return Manifest{}, fmt.Errorf("read skill bundle manifest: %w", err)
	}
	if len(manifest.Files) == 0 {
		return Manifest{}, ErrNotFound
	}
	return manifest, nil
}

// Blob returns the body of one file of the org's bundle for the skill. A digest the skill's
// bundle does not name is ErrNotFound, even when another skill of the org stores the same
// bytes, so a read is bound to the bundle it was asked about.
func (s *Store) Blob(ctx context.Context, orgID, skillID, sha256 string) ([]byte, error) {
	var content []byte
	err := s.db.QueryRow(ctx, `
		SELECT b.content
		FROM skill_bundle_blobs b
		WHERE b.org_id = $1 AND b.sha256 = $3
		  AND EXISTS (
		      SELECT 1 FROM skill_bundle_files f
		      JOIN skill_bundles sb ON sb.id = f.bundle_id
		      WHERE sb.org_id = $1 AND sb.skill_id = $2 AND f.sha256 = $3 AND sb.state <> 'draft'
		  )
	`, orgID, skillID, sha256).Scan(&content)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, fmt.Errorf("read skill blob: %w", err)
	}
	return content, nil
}
