// Package docstore keeps what a company uploaded as tender documents for a notice: the manifest of
// the archive and the passages of the documents that name a certificate or a registration the gate
// knows. It does not keep the archive, the files or their text: those are read while the archive is
// uploaded and dropped (docs/ARCHITECTURE.md, "Tender documents").
//
// It is tenant data, in the schema tenant (migration 0007), and not evidence: an upload is deleted
// on request and when it expires. Every call names the organisation, every query filters by it, and
// the database refuses a row of another organisation whatever a query forgets, through row level
// security that this store switches to the session's organisation with set_config at the start of
// each transaction.
package docstore

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"regexp"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

// Limits. The table checks most of them again, so a bug here cannot write what the table forbids.
const (
	MaxDocuments       = 500
	MaxFindings        = 400
	MaxSnippetRunes    = 600
	MaxEvidenceNotices = 500
	MaxRetention       = 2 * 365 * 24 * time.Hour
	// MinRetention is the shortest an upload is kept. The database adds the retention to the time of the insert as
	// an interval, which it keeps to the microsecond, and a retention shorter than that is nothing: the upload would
	// expire the moment it was made, which the table refuses, and every upload would fail.
	MinRetention      = time.Second
	maxPathRunes      = 240
	maxArchiveNameLen = 200
	// MaxEncodedBytes is the most the manifest, and the most the findings, may come to once encoded: the
	// table refuses more than 1 MiB of the text of either, and the text of a jsonb value is a little
	// longer than the JSON it was made from. A refusal here is a refusal that names the field, and not a
	// check failing in the database.
	MaxEncodedBytes = 1_000_000
)

// The statuses a document of an archive ends with.
const (
	StatusRead        = "read"        // its text was read
	StatusEmpty       = "empty"       // it was read and holds no text: a scan, which needs OCR
	StatusUnsupported = "unsupported" // a kind of file that is not read
	StatusFailed      = "failed"      // the file is damaged, or reading it failed
	StatusEncrypted   = "encrypted"   // it asks for a password
	StatusTooLarge    = "too_large"   // it is larger than a file may be
	StatusSkipped     = "skipped"     // it was left out, for the reason given
	StatusDuplicate   = "duplicate"   // the same bytes as another file of the archive
)

// ErrInvalid means the store refused a value before it asked the database: an organisation or a
// notice that is not one, a hash that is not a hash, a list that is too long. The message says which
// field and never quotes the value.
var ErrInvalid = errors.New("docstore: invalid value")

var (
	orgPattern     = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$`)
	hashPattern    = regexp.MustCompile(`^[0-9a-f]{64}$`)
	kindPattern    = regexp.MustCompile(`^[a-z0-9]{1,20}$`)
	reasonPattern  = regexp.MustCompile(`^[a-z_]{0,40}$`)
	capabilityName = regexp.MustCompile(`^[a-z0-9_]{1,40}$`)
)

var (
	validStatus = map[string]bool{
		StatusRead: true, StatusEmpty: true, StatusUnsupported: true, StatusFailed: true, StatusEncrypted: true,
		StatusTooLarge: true, StatusSkipped: true, StatusDuplicate: true,
	}
	// unreadStatus are the statuses of a file that was in the archive and could not be used. A
	// duplicate is not one: its bytes were read once, as another file.
	unreadStatus = map[string]bool{
		StatusEmpty: true, StatusUnsupported: true, StatusFailed: true, StatusEncrypted: true,
		StatusTooLarge: true, StatusSkipped: true,
	}
	validClass = map[string]bool{"requirement": true, "award": true, "negated": true, "mention": true}
)

// Document is one file of an uploaded archive, as the manifest lists it.
type Document struct {
	Path string `json:"path"`
	Kind string `json:"kind"`
	Size int64  `json:"size"`
	// SHA256 is the hash of the file's bytes. A file that was skipped was never read and has none.
	SHA256    string `json:"sha256,omitempty"`
	Status    string `json:"status"`
	Reason    string `json:"reason,omitempty"` // for a skipped file, a fixed code
	Pages     int    `json:"pages,omitempty"`
	Chars     int    `json:"chars,omitempty"`
	Truncated bool   `json:"truncated,omitempty"`
	Extractor string `json:"extractor,omitempty"`
}

// Finding is one sentence of one document that names a capability, with where it is.
type Finding struct {
	Document   string `json:"document"`
	Capability string `json:"capability"`
	Class      string `json:"class"`
	Page       int    `json:"page,omitempty"`
	Paragraph  int    `json:"paragraph,omitempty"`
	Snippet    string `json:"snippet"`
}

// NewUpload is an upload to store.
type NewUpload struct {
	OrgID         string
	NoticeID      string
	UploadedBy    string // a user id, or ""
	ArchiveName   string // as sent, with control characters removed, or ""
	ArchiveSHA256 string
	ArchiveBytes  int64
	Documents     []Document
	Findings      []Finding
	// FindingsDropped is how many passages were left out of Findings because there are only so many
	// an upload may keep.
	FindingsDropped int
	// Retention is how long it is kept: at least MinRetention and at most MaxRetention.
	Retention time.Duration
	// MaxPerNotice and MaxPerOrg are the most uploads the organisation may hold, for the notice and in all, this one
	// included; 0 is no limit. Create enforces them itself, in the transaction that stores the upload and under a lock
	// of the organisation, so that uploads that are made at once cannot pass them. A count made before, which is what
	// Counts is for, is passed by every upload that is under way at once.
	MaxPerNotice, MaxPerOrg int
}

// Upload is a stored upload.
type Upload struct {
	ID            int64
	OrgID         string
	NoticeID      string
	UploadedBy    string
	ArchiveName   string
	ArchiveSHA256 string
	ArchiveBytes  int64
	Documents     []Document
	Findings      []Finding
	ReadCount     int
	UnreadCount   int
	// FindingsDropped is how many passages were left out of Findings when it was stored.
	FindingsDropped int
	CreatedAt       time.Time
	ExpiresAt       time.Time
}

// Store is the tenant document store.
type Store struct {
	pool *pgxpool.Pool
	// evidenceBytes is what one call to EvidenceFor may hold on to of the findings it reads (MaxEvidenceBytes).
	evidenceBytes int
}

// New returns a Store over pool. It does not check the pool.
func New(pool *pgxpool.Pool) *Store { return &Store{pool: pool, evidenceBytes: MaxEvidenceBytes} }

func invalid(field string) error { return fmt.Errorf("%w: %s", ErrInvalid, field) }

// QuotaError means the organisation already holds as many uploads as it may, for the notice or in all. Scope is
// "notice" or "organisation". Create returns it when the limits it was given (NewUpload.MaxPerNotice and MaxPerOrg)
// are reached.
type QuotaError struct{ Scope string }

func (e *QuotaError) Error() string {
	return "docstore: the organisation already holds as many uploads as it may for the " + e.Scope
}

func checkOrg(org string) error {
	if !orgPattern.MatchString(org) {
		return invalid("organisation")
	}
	return nil
}

func checkNotice(id string) error {
	if !doffin.IsNoticeID(id) {
		return invalid("notice id")
	}
	return nil
}

// text reports whether s is text the database can hold: valid UTF-8 with no NUL.
func text(s string) bool { return utf8.ValidString(s) && !strings.ContainsRune(s, 0) }

func (u NewUpload) validate() error {
	switch {
	case checkOrg(u.OrgID) != nil:
		return invalid("organisation")
	case checkNotice(u.NoticeID) != nil:
		return invalid("notice id")
	case u.UploadedBy != "" && !orgPattern.MatchString(u.UploadedBy):
		return invalid("uploaded by")
	case !hashPattern.MatchString(u.ArchiveSHA256):
		return invalid("archive hash")
	case u.ArchiveBytes <= 0:
		return invalid("archive size")
	case utf8.RuneCountInString(u.ArchiveName) > maxArchiveNameLen || !text(u.ArchiveName):
		return invalid("archive name")
	case u.Retention < MinRetention || u.Retention > MaxRetention:
		return invalid("retention")
	case u.MaxPerNotice < 0 || u.MaxPerOrg < 0:
		return invalid("limits")
	case len(u.Documents) > MaxDocuments:
		return invalid("documents")
	case len(u.Findings) > MaxFindings:
		return invalid("findings")
	case u.FindingsDropped < 0:
		return invalid("findings dropped")
	}
	for _, d := range u.Documents {
		switch {
		case d.Path == "" || utf8.RuneCountInString(d.Path) > maxPathRunes || !text(d.Path):
			return invalid("document path")
		case !kindPattern.MatchString(d.Kind):
			return invalid("document kind")
		case d.Size < 0 || d.Pages < 0 || d.Chars < 0:
			return invalid("document size")
		case !hashPattern.MatchString(d.SHA256) && !(d.Status == StatusSkipped && d.SHA256 == ""):
			return invalid("document hash")
		case !validStatus[d.Status]:
			return invalid("document status")
		case !reasonPattern.MatchString(d.Reason):
			return invalid("document reason")
		case len(d.Extractor) > 20 || !text(d.Extractor):
			return invalid("document extractor")
		}
	}
	for _, f := range u.Findings {
		switch {
		case f.Document == "" || utf8.RuneCountInString(f.Document) > maxPathRunes || !text(f.Document):
			return invalid("finding document")
		case !capabilityName.MatchString(f.Capability):
			return invalid("finding capability")
		case !validClass[f.Class]:
			return invalid("finding class")
		case f.Page < 0 || f.Paragraph < 0:
			return invalid("finding place")
		case f.Snippet == "" || utf8.RuneCountInString(f.Snippet) > MaxSnippetRunes || !text(f.Snippet):
			return invalid("finding snippet")
		}
	}
	return nil
}

// encode is JSON without the escaping of <, > and &, which the database does not do and which would
// make a sentence full of them six times as long in the size that is checked.
func encode(v any) ([]byte, error) {
	var buf bytes.Buffer
	enc := json.NewEncoder(&buf)
	enc.SetEscapeHTML(false)
	if err := enc.Encode(v); err != nil {
		return nil, err
	}
	return bytes.TrimRight(buf.Bytes(), "\n"), nil
}

// Encoded returns the manifest and the findings of the upload as the JSON that is stored, and refuses
// either when it is larger than MaxEncodedBytes. It is what Create stores, and is exported so that a
// producer can check what it built before it asks the database.
func (u NewUpload) Encoded() (documents, findings []byte, err error) {
	if documents, err = encode(nonNilDocuments(u.Documents)); err != nil {
		return nil, nil, fmt.Errorf("docstore: encode the manifest: %w", err)
	}
	if findings, err = encode(nonNilFindings(u.Findings)); err != nil {
		return nil, nil, fmt.Errorf("docstore: encode the findings: %w", err)
	}
	switch {
	case len(documents) > MaxEncodedBytes:
		return nil, nil, invalid("manifest is too large")
	case len(findings) > MaxEncodedBytes:
		return nil, nil, invalid("findings are too large")
	}
	return documents, findings, nil
}

// FindingBytes is how many bytes a finding takes in the encoded list of the findings of an upload, with the comma
// that follows it. The list of n findings is the brackets and the n of these without the last comma, so it is within
// MaxEncodedBytes when 1 and what this comes to for each of them are: what a producer needs to keep it so.
func FindingBytes(f Finding) (int, error) {
	b, err := encode(f)
	if err != nil {
		return 0, fmt.Errorf("docstore: encode a finding: %w", err)
	}
	return len(b) + 1, nil
}

// counts says how many of the documents were read and how many could not be.
func counts(docs []Document) (read, unread int) {
	for _, d := range docs {
		switch {
		case d.Status == StatusRead:
			read++
		case unreadStatus[d.Status]:
			unread++
		}
	}
	return read, unread
}

// scoped runs fn in a transaction whose session names the organisation, which is what the row level
// policy compares each row with. The setting lasts for the transaction and no longer.
func (s *Store) scoped(ctx context.Context, org string, fn func(tx pgx.Tx) error) error {
	// READ COMMITTED, whatever the role's sessions default to: each statement sees what is committed when it begins.
	// Create counts after it has waited for the lock of the organisation, and that is only worth anything if the count
	// sees what the upload that held the lock before it committed, which a snapshot taken before the wait does not.
	tx, err := s.pool.BeginTx(ctx, pgx.TxOptions{IsoLevel: pgx.ReadCommitted})
	if err != nil {
		return fmt.Errorf("docstore: begin: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	if _, err := tx.Exec(ctx, `SELECT set_config('anbud.org_id', $1, true)`, org); err != nil {
		return fmt.Errorf("docstore: set the organisation: %w", err)
	}
	if err := fn(tx); err != nil {
		return err
	}
	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("docstore: commit: %w", err)
	}
	return nil
}

const uploadColumns = `id, org_id, notice_id, coalesce(uploaded_by, ''), archive_name, archive_sha256, archive_bytes,
documents, findings, read_count, unread_count, findings_dropped, created_at, expires_at`

const insertUpload = `
INSERT INTO tenant.tender_uploads
    (org_id, notice_id, uploaded_by, archive_name, archive_sha256, archive_bytes, documents, findings, read_count, unread_count,
     findings_dropped, expires_at)
VALUES ($1, $2, nullif($3, ''), $4, $5, $6, $7, $8, $9, $10, $11, now() + make_interval(secs => $12))
ON CONFLICT ON CONSTRAINT tender_uploads_once DO NOTHING
RETURNING ` + uploadColumns

// An upload that has expired is a row until it is purged, and it holds the name of its archive. It is
// removed first, so that the same archive can be uploaded again without waiting for the purge.
const deleteExpiredSame = `
DELETE FROM tenant.tender_uploads
 WHERE org_id = $1 AND notice_id = $2 AND archive_sha256 = $3 AND expires_at <= now()`

const selectSame = `
SELECT ` + uploadColumns + `
  FROM tenant.tender_uploads
 WHERE org_id = $1 AND notice_id = $2 AND archive_sha256 = $3 AND expires_at > now()`

// quotaLockClass is the first key of the advisory lock that Create takes, and the organisation is the second. The
// lock with two keys is a space of its own, apart from the locks with one key that the sync and the migrations take
// (internal/rawstore, internal/db), so that none of them can be the same lock by chance. The number is "TDU" and a
// zero byte.
const quotaLockClass = 0x54445500

// lockOrganisation takes the lock of an organisation's uploads, for the rest of the transaction: uploads of one
// organisation are stored one at a time, so that each counts what the one before it stored. A transaction that
// ends, however it ends, lets the lock go.
const lockOrganisation = `SELECT pg_advisory_xact_lock($1::int4, hashtext($2))`

const countUploads = `
SELECT count(*) FILTER (WHERE notice_id = $2), count(*)
  FROM tenant.tender_uploads
 WHERE org_id = $1 AND expires_at > now()`

const selectForNotice = `
SELECT ` + uploadColumns + `
  FROM tenant.tender_uploads
 WHERE org_id = $1 AND notice_id = $2 AND expires_at > now()
 ORDER BY created_at, id`

// Create stores an upload. The same archive for the same notice by the same organisation is one
// upload: asked again, it returns the one it holds and false. When the upload has limits (MaxPerNotice, MaxPerOrg)
// they are enforced here, in the transaction that stores it: the organisation's lock is taken first, so that the
// uploads of one organisation are counted one after another, and an organisation that holds as many as it may gets a
// *QuotaError and nothing is stored. The same archive is looked for before anything is counted, so an organisation at
// its limit that sends what it has is told it has it.
func (s *Store) Create(ctx context.Context, in NewUpload) (Upload, bool, error) {
	if err := in.validate(); err != nil {
		return Upload{}, false, err
	}
	docs, findings, err := in.Encoded()
	if err != nil {
		return Upload{}, false, err
	}
	read, unread := counts(in.Documents)

	var up Upload
	var created bool
	limited := in.MaxPerNotice > 0 || in.MaxPerOrg > 0
	err = s.scoped(ctx, in.OrgID, func(tx pgx.Tx) error {
		if limited {
			if _, err := tx.Exec(ctx, lockOrganisation, int32(quotaLockClass), in.OrgID); err != nil {
				return fmt.Errorf("docstore: lock the uploads of the organisation: %w", err)
			}
		}
		if _, err := tx.Exec(ctx, deleteExpiredSame, in.OrgID, in.NoticeID, in.ArchiveSHA256); err != nil {
			return fmt.Errorf("docstore: remove the expired upload of the same archive: %w", err)
		}
		if limited {
			same, found, err := checkLimits(ctx, tx, in)
			if err != nil {
				return err
			}
			if found {
				up = same
				return nil
			}
		}
		got, err := scanUpload(tx.QueryRow(ctx, insertUpload,
			in.OrgID, in.NoticeID, in.UploadedBy, in.ArchiveName, in.ArchiveSHA256, in.ArchiveBytes,
			docs, findings, read, unread, in.FindingsDropped, in.Retention.Seconds()))
		if err == nil {
			up, created = got, true
			return nil
		}
		if !errors.Is(err, pgx.ErrNoRows) {
			return fmt.Errorf("docstore: store the upload: %w", err)
		}
		// The same archive is stored and has not expired: that is the upload.
		same, err := scanUpload(tx.QueryRow(ctx, selectSame, in.OrgID, in.NoticeID, in.ArchiveSHA256))
		if err != nil {
			return fmt.Errorf("docstore: find the upload that was stored before: %w", err)
		}
		up = same
		return nil
	})
	return up, created, err
}

// checkLimits is what Create does, in its transaction and with the organisation locked, before it stores an upload that
// has limits. The same archive is looked for first: it is the upload that is there, and is not refused for a limit it is
// part of. What is left is counted, the notice before the organisation, and refused when the limit is reached.
func checkLimits(ctx context.Context, tx pgx.Tx, in NewUpload) (same Upload, found bool, err error) {
	same, err = scanUpload(tx.QueryRow(ctx, selectSame, in.OrgID, in.NoticeID, in.ArchiveSHA256))
	switch {
	case err == nil:
		return same, true, nil
	case !errors.Is(err, pgx.ErrNoRows):
		return Upload{}, false, fmt.Errorf("docstore: look for the same archive: %w", err)
	}
	var forNotice, forOrg int
	if err := tx.QueryRow(ctx, countUploads, in.OrgID, in.NoticeID).Scan(&forNotice, &forOrg); err != nil {
		return Upload{}, false, fmt.Errorf("docstore: count the uploads: %w", err)
	}
	switch {
	case in.MaxPerNotice > 0 && forNotice >= in.MaxPerNotice:
		return Upload{}, false, &QuotaError{Scope: "notice"}
	case in.MaxPerOrg > 0 && forOrg >= in.MaxPerOrg:
		return Upload{}, false, &QuotaError{Scope: "organisation"}
	}
	return Upload{}, false, nil
}

// FindSame returns the upload of the same archive for the notice, when the organisation holds one that
// has not expired. It reads one row and no more, which is what a repeated upload needs to know.
func (s *Store) FindSame(ctx context.Context, org, noticeID, archiveSHA256 string) (Upload, bool, error) {
	if err := checkOrg(org); err != nil {
		return Upload{}, false, err
	}
	if err := checkNotice(noticeID); err != nil {
		return Upload{}, false, err
	}
	if !hashPattern.MatchString(archiveSHA256) {
		return Upload{}, false, invalid("archive hash")
	}
	var up Upload
	var found bool
	err := s.scoped(ctx, org, func(tx pgx.Tx) error {
		got, err := scanUpload(tx.QueryRow(ctx, selectSame, org, noticeID, archiveSHA256))
		switch {
		case errors.Is(err, pgx.ErrNoRows):
			return nil
		case err != nil:
			return fmt.Errorf("docstore: find the same archive: %w", err)
		}
		up, found = got, true
		return nil
	})
	return up, found, err
}

// List returns the uploads of one notice that the organisation holds and that have not expired,
// oldest first.
func (s *Store) List(ctx context.Context, org, noticeID string) ([]Upload, error) {
	if err := checkOrg(org); err != nil {
		return nil, err
	}
	if err := checkNotice(noticeID); err != nil {
		return nil, err
	}
	var out []Upload
	err := s.scoped(ctx, org, func(tx pgx.Tx) error {
		rows, err := tx.Query(ctx, selectForNotice, org, noticeID)
		if err != nil {
			return fmt.Errorf("docstore: list: %w", err)
		}
		defer rows.Close()
		for rows.Next() {
			up, err := scanUpload(rows)
			if err != nil {
				return err
			}
			out = append(out, up)
		}
		return rows.Err()
	})
	return out, err
}

// Delete removes an upload of the organisation's own, through the notice it was uploaded for, and says
// whether there was one: the id of an upload of another notice is not found. It is one statement, so
// there is no gap between finding the upload and deleting it.
func (s *Store) Delete(ctx context.Context, org, noticeID string, id int64) (bool, error) {
	if err := checkOrg(org); err != nil {
		return false, err
	}
	if err := checkNotice(noticeID); err != nil {
		return false, err
	}
	if id <= 0 {
		return false, invalid("upload id")
	}
	var deleted bool
	err := s.scoped(ctx, org, func(tx pgx.Tx) error {
		tag, err := tx.Exec(ctx, `DELETE FROM tenant.tender_uploads WHERE org_id = $1 AND notice_id = $2 AND id = $3`, org, noticeID, id)
		if err != nil {
			return fmt.Errorf("docstore: delete: %w", err)
		}
		deleted = tag.RowsAffected() > 0
		return nil
	})
	return deleted, err
}

// Counts returns how many live uploads the organisation holds for the notice, and in all.
func (s *Store) Counts(ctx context.Context, org, noticeID string) (forNotice, forOrg int, err error) {
	if err := checkOrg(org); err != nil {
		return 0, 0, err
	}
	if err := checkNotice(noticeID); err != nil {
		return 0, 0, err
	}
	err = s.scoped(ctx, org, func(tx pgx.Tx) error {
		return tx.QueryRow(ctx, countUploads, org, noticeID).Scan(&forNotice, &forOrg)
	})
	return forNotice, forOrg, err
}

// PurgeExpired removes every upload whose expiry has passed, for every organisation, and returns how
// many. It runs a function that is not held to the row policy (migration 0007) and does nothing else.
func (s *Store) PurgeExpired(ctx context.Context) (int64, error) {
	var removed int64
	if err := s.pool.QueryRow(ctx, `SELECT tenant.purge_expired_uploads()`).Scan(&removed); err != nil {
		return 0, fmt.Errorf("docstore: purge: %w", err)
	}
	return removed, nil
}

// unreadKey says which file an unread document is: the hash of its bytes, or, for a file that was
// skipped and so has none, its path.
func unreadKey(d Document) string {
	if d.SHA256 == "" {
		return "path:" + d.Path
	}
	return d.SHA256
}

func nonNilDocuments(d []Document) []Document {
	if d == nil {
		return []Document{}
	}
	return d
}

func nonNilFindings(f []Finding) []Finding {
	if f == nil {
		return []Finding{}
	}
	return f
}

type rowScanner interface {
	Scan(dest ...any) error
}

func scanUpload(row rowScanner) (Upload, error) {
	var u Upload
	var docs, findings []byte
	if err := row.Scan(&u.ID, &u.OrgID, &u.NoticeID, &u.UploadedBy, &u.ArchiveName, &u.ArchiveSHA256, &u.ArchiveBytes,
		&docs, &findings, &u.ReadCount, &u.UnreadCount, &u.FindingsDropped, &u.CreatedAt, &u.ExpiresAt); err != nil {
		return Upload{}, err
	}
	if err := json.Unmarshal(docs, &u.Documents); err != nil {
		return Upload{}, fmt.Errorf("docstore: decode a manifest: %w", err)
	}
	if err := json.Unmarshal(findings, &u.Findings); err != nil {
		return Upload{}, fmt.Errorf("docstore: decode findings: %w", err)
	}
	u.CreatedAt, u.ExpiresAt = u.CreatedAt.UTC(), u.ExpiresAt.UTC()
	return u, nil
}
