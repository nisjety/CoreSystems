// Package tenderdocs takes the tender documents a company already holds for a notice, as a ZIP
// archive, reads them, and keeps what the gate can use: the manifest of the archive and the sentences
// of its documents that name a certificate or a registration the rules know. It does not keep the
// archive, the files or their text.
//
// The documents are the company's own: downloaded from Mercell or EU-Supply by a person who was
// allowed to, and uploaded here. Nothing here crawls a platform or logs in to one.
//
// An upload is read while the request is open, in memory, and dropped. What is kept is tenant data in
// the schema tenant (docstore): it belongs to one organisation, it expires, and it is deleted on
// request. The text of a document is not kept, and the log says how many files and findings an upload
// held and never what they were called or said.
//
// What is read from a document is advice. The gate treats it as it treats the notice's own selection
// criteria, but it can make a check conditional and never failed: a sentence found by keyword is not
// a requirement proved, and a person reads it (internal/match).
package tenderdocs

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/rs/zerolog"

	"github.com/triodelab/anbud/internal/docread"
	"github.com/triodelab/anbud/internal/docstore"
	"github.com/triodelab/anbud/internal/sources/doffin"
	"github.com/triodelab/anbud/internal/textsafe"
	"github.com/triodelab/anbud/internal/ziputil"
)

// Defaults. The limits that bound memory are set by the operator (ANBUD_TENDER_DOCS_MAX_MB): one upload
// holds the archive and one expanded file at a time, and at most MaxConcurrent uploads run.
const (
	DefaultRetention       = 90 * 24 * time.Hour
	DefaultMaxArchiveBytes = 100 << 20

	defaultMaxConcurrent = 2
	defaultTimeout       = 3 * time.Minute
	// defaultStoreTimeout bounds each call to the database. A call that hangs (a connection that is half
	// open, say) must not hold a request, and so a slot, for ever.
	defaultStoreTimeout = 30 * time.Second
	defaultMaxPerNotice = 10
	defaultMaxPerOrg    = 200

	maxArchiveNameRunes = 200
)

// DefaultLimits are what an archive of tender documents may be: the archive itself is maxArchive
// bytes, and no file in it is larger than a PDF the reader takes.
func DefaultLimits(maxArchive int64) ziputil.Limits {
	return ziputil.Limits{
		MaxArchiveBytes: maxArchive,
		MaxEntries:      2000,
		MaxTotalEntries: 8000,
		MaxEntryBytes:   docread.DefaultLimits.MaxPDFBytes,
		MaxTotalBytes:   1 << 30,
		MaxRatio:        200,
		MaxDepth:        3,
	}
}

// The reasons an archive is refused. They are fixed codes for the caller, never a value from the
// archive.
const (
	ReasonEmpty          = "empty"            // the body holds nothing
	ReasonNotAZip        = "not_a_zip"        // it is not a ZIP archive this code can read
	ReasonTooLarge       = "too_large"        // the archive, or what it expands to, is over a limit
	ReasonTooManyEntries = "too_many_entries" // its directory names more entries than allowed
	ReasonTooManyFiles   = "too_many_files"   // it holds more files than a manifest can list
	ReasonExpandsTooFar  = "expands_too_far"  // a file expands far beyond its size: a zip bomb
)

var (
	// ErrZDR means the organisation is in zero-data-retention mode: nothing it uploads is stored, and
	// an upload exists to be stored.
	ErrZDR = errors.New("tenderdocs: zero data retention: tender documents are not stored")
	// ErrInvalidNotice means the id does not have the shape of a notice id.
	ErrInvalidNotice = errors.New("tenderdocs: invalid notice id")
	// ErrNoticeNotFound means the notice is not held: documents are uploaded for a notice the service has.
	ErrNoticeNotFound = errors.New("tenderdocs: the notice is not held")
	// ErrNotFound means the organisation holds no such upload.
	ErrNotFound = errors.New("tenderdocs: upload not found")
	// ErrBusy means every slot for reading an archive is taken. Nothing was read; try again shortly.
	ErrBusy = errors.New("tenderdocs: too many archives are being read")
	// ErrTimeout means reading the archive took too long. Nothing was stored.
	ErrTimeout = errors.New("tenderdocs: reading the archive took too long")
)

// ArchiveError says why an archive was refused. Reason is one of the Reason constants.
type ArchiveError struct{ Reason string }

func (e *ArchiveError) Error() string { return "tenderdocs: the archive is refused: " + e.Reason }

// QuotaError means the organisation already holds as many uploads as it may. Scope is "notice" or
// "organisation".
type QuotaError struct{ Scope string }

func (e *QuotaError) Error() string {
	return "tenderdocs: the upload limit of the " + e.Scope + " is reached"
}

// Store is the tenant document store, which is docstore.Store.
type Store interface {
	Create(ctx context.Context, in docstore.NewUpload) (docstore.Upload, bool, error)
	FindSame(ctx context.Context, org, noticeID, archiveSHA256 string) (docstore.Upload, bool, error)
	List(ctx context.Context, org, noticeID string) ([]docstore.Upload, error)
	Delete(ctx context.Context, org, noticeID string, id int64) (bool, error)
	Counts(ctx context.Context, org, noticeID string) (forNotice, forOrg int, err error)
	EvidenceFor(ctx context.Context, org string, noticeIDs []string) (map[string]docstore.Evidence, error)
	PurgeExpired(ctx context.Context) (int64, error)
}

// Notices says whether a notice is held, which is rawstore.Store.
type Notices interface {
	HasNotice(ctx context.Context, id string) (bool, error)
}

// Reader reads one document, which is docread.Reader.
type Reader interface {
	Read(ctx context.Context, data []byte) (docread.Content, error)
}

// Options configures a Service.
type Options struct {
	Store   Store
	Notices Notices
	Reader  Reader
	// Limits bound an archive. The zero value means DefaultLimits(DefaultMaxArchiveBytes).
	Limits ziputil.Limits
	// Retention is how long an upload is kept: at least docstore.MinRetention and at most docstore.MaxRetention.
	Retention time.Duration
	// MaxConcurrent bounds the archives read at once; two when zero.
	MaxConcurrent int
	// Timeout bounds reading one archive; three minutes when zero.
	Timeout time.Duration
	// StoreTimeout bounds each call to the database; 30 seconds when zero.
	StoreTimeout time.Duration
	// MaxUploadsPerNotice and MaxUploadsPerOrganisation bound what an organisation keeps; 10 and 200
	// when zero.
	MaxUploadsPerNotice       int
	MaxUploadsPerOrganisation int
	Logger                    *zerolog.Logger // optional
}

// Service reads uploaded archives and answers what they said. It keeps no state of its own: it is safe
// for concurrent use.
type Service struct {
	store     Store
	notices   Notices
	reader    Reader
	limits    ziputil.Limits
	retention time.Duration
	slots     chan struct{}
	timeout   time.Duration
	storeWait time.Duration
	perNotice int
	perOrg    int
	log       zerolog.Logger
}

// New builds a Service. It refuses options that cannot work, so that a mistake stops the service at
// start-up and not at the first upload.
func New(o Options) (*Service, error) {
	switch {
	case o.Store == nil:
		return nil, errors.New("tenderdocs: a store is required")
	case o.Notices == nil:
		return nil, errors.New("tenderdocs: a notice store is required")
	case o.Reader == nil:
		return nil, errors.New("tenderdocs: a document reader is required")
	case o.Retention < docstore.MinRetention || o.Retention > docstore.MaxRetention:
		return nil, fmt.Errorf("tenderdocs: the retention must be at least %s and at most %s", docstore.MinRetention, docstore.MaxRetention)
	}
	lim := o.Limits
	if lim == (ziputil.Limits{}) {
		lim = DefaultLimits(DefaultMaxArchiveBytes)
	}
	if lim.MaxArchiveBytes <= 0 || lim.MaxEntries <= 0 || lim.MaxEntryBytes <= 0 || lim.MaxTotalBytes <= 0 || lim.MaxRatio <= 0 || lim.MaxDepth <= 0 {
		return nil, errors.New("tenderdocs: every archive limit must be more than nothing")
	}
	s := &Service{
		store: o.Store, notices: o.Notices, reader: o.Reader, limits: lim, retention: o.Retention,
		timeout: o.Timeout, perNotice: o.MaxUploadsPerNotice, perOrg: o.MaxUploadsPerOrganisation, log: zerolog.Nop(),
	}
	if o.Logger != nil {
		s.log = *o.Logger
	}
	if s.timeout <= 0 {
		s.timeout = defaultTimeout
	}
	s.storeWait = o.StoreTimeout
	if s.storeWait <= 0 {
		s.storeWait = defaultStoreTimeout
	}
	if s.perNotice <= 0 {
		s.perNotice = defaultMaxPerNotice
	}
	if s.perOrg <= 0 {
		s.perOrg = defaultMaxPerOrg
	}
	slots := o.MaxConcurrent
	if slots <= 0 {
		slots = defaultMaxConcurrent
	}
	s.slots = make(chan struct{}, slots)
	return s, nil
}

// MaxArchiveBytes is the largest archive an upload may be. The HTTP layer uses it to size the body it
// accepts.
func (s *Service) MaxArchiveBytes() int64 { return s.limits.MaxArchiveBytes }

// UploadRequest is an archive and who sent it, for which notice.
type UploadRequest struct {
	OrgID       string
	UserID      string // who sent it, or ""
	NoticeID    string
	ArchiveName string // as the sender called it; cleaned before it is kept
	// ZDR is true when the request or the organisation is in zero-data-retention mode.
	ZDR     bool
	Archive []byte
}

// Upload reads an archive and stores its manifest and findings, and says whether it stored anything
// new: the same archive for the same notice is one upload, and sent again it is neither read nor
// stored again. The archive is read in memory and nothing of it is written anywhere. Nothing is stored
// unless the whole archive was read.
func (s *Service) Upload(ctx context.Context, r UploadRequest) (docstore.Upload, bool, error) {
	switch {
	case r.ZDR:
		return docstore.Upload{}, false, ErrZDR
	case !doffin.IsNoticeID(r.NoticeID):
		return docstore.Upload{}, false, ErrInvalidNotice
	case len(r.Archive) == 0:
		return docstore.Upload{}, false, &ArchiveError{Reason: ReasonEmpty}
	case int64(len(r.Archive)) > s.limits.MaxArchiveBytes:
		return docstore.Upload{}, false, &ArchiveError{Reason: ReasonTooLarge}
	}
	sctx, cancel := s.bounded(ctx)
	held, err := s.notices.HasNotice(sctx, r.NoticeID)
	cancel()
	if err != nil {
		return docstore.Upload{}, false, fmt.Errorf("tenderdocs: look for the notice: %w", err)
	}
	if !held {
		return docstore.Upload{}, false, ErrNoticeNotFound
	}

	sum := sha256.Sum256(r.Archive)
	archiveSHA := hex.EncodeToString(sum[:])

	// Before any work: the same archive again is the upload it already is, and an organisation that
	// holds as many as it may is told so. The store checks the organisation's id on the way.
	sctx, cancel = s.bounded(ctx)
	same, found, err := s.store.FindSame(sctx, r.OrgID, r.NoticeID, archiveSHA)
	cancel()
	if err != nil {
		return docstore.Upload{}, false, fmt.Errorf("tenderdocs: look for the same archive: %w", err)
	}
	if found {
		return same, false, nil
	}
	// A first count, so that an archive that cannot be kept is not read. It is not the limit: every upload that is
	// under way at once passes it, and the store counts again, for good, when it stores the upload (below).
	sctx, cancel = s.bounded(ctx)
	forNotice, forOrg, err := s.store.Counts(sctx, r.OrgID, r.NoticeID)
	cancel()
	if err != nil {
		return docstore.Upload{}, false, fmt.Errorf("tenderdocs: count the uploads: %w", err)
	}
	switch {
	case forNotice >= s.perNotice:
		return docstore.Upload{}, false, &QuotaError{Scope: "notice"}
	case forOrg >= s.perOrg:
		return docstore.Upload{}, false, &QuotaError{Scope: "organisation"}
	}

	start := time.Now()
	out, err := s.read(ctx, r.Archive)
	if err != nil {
		return docstore.Upload{}, false, err
	}

	// The slot is given back: it is for reading, and what is left is a call to the database.
	sctx, cancel = s.bounded(ctx)
	defer cancel()
	up, created, err := s.store.Create(sctx, docstore.NewUpload{
		OrgID: r.OrgID, NoticeID: r.NoticeID, UploadedBy: r.UserID, ArchiveName: cleanName(r.ArchiveName),
		ArchiveSHA256: archiveSHA, ArchiveBytes: int64(len(r.Archive)),
		Documents: out.documents, Findings: out.findings, FindingsDropped: out.dropped, Retention: s.retention,
		// The store counts again, in the transaction that stores the upload and under a lock of the organisation. The
		// count above is passed by every upload that is under way at once; this one is not.
		MaxPerNotice: s.perNotice, MaxPerOrg: s.perOrg,
	})
	var overLimit *docstore.QuotaError
	if errors.As(err, &overLimit) {
		// The last place went to an upload that was stored after the count above: refused as it would have been had it
		// been counted a moment later, and its archive was read for nothing.
		return docstore.Upload{}, false, &QuotaError{Scope: overLimit.Scope}
	}
	if errors.Is(err, docstore.ErrInvalid) {
		// Everything that went into the upload was checked on the way in, so a store that refuses what was
		// built from it has found a fault here, not in the request: the caller is not told it was theirs.
		return docstore.Upload{}, false, fmt.Errorf("tenderdocs: the store refused an upload that was built from valid parts (a bug): %s", err)
	}
	if err != nil {
		return docstore.Upload{}, false, fmt.Errorf("tenderdocs: store the upload: %w", err)
	}
	// Counts only: the names of the files and the sentences they hold are the company's.
	s.log.Info().
		Int("files", len(out.documents)).Int("read", up.ReadCount).Int("unread", up.UnreadCount).
		Int("findings", len(out.findings)).Int("findings_dropped", out.dropped).
		Bool("created", created).Dur("took", time.Since(start)).
		Msg("tender documents uploaded")
	return up, created, nil
}

// read reads the archive while holding one of the slots for reading, and gives the slot back whatever
// came of it. The deadline is the archive's own: a caller that gives up is not a timeout.
func (s *Service) read(ctx context.Context, archive []byte) (processed, error) {
	release, err := s.acquire()
	if err != nil {
		return processed{}, err
	}
	defer release()

	pctx, cancel := context.WithTimeout(ctx, s.timeout)
	defer cancel()
	out, err := s.process(pctx, archive)
	if err != nil {
		// Our own deadline is a timeout; the caller's cancel is the caller's own.
		if ctx.Err() == nil && errors.Is(pctx.Err(), context.DeadlineExceeded) {
			return processed{}, ErrTimeout
		}
		return processed{}, err
	}
	return out, nil
}

// bounded is ctx with the deadline of one call to the database.
func (s *Service) bounded(ctx context.Context) (context.Context, context.CancelFunc) {
	return context.WithTimeout(ctx, s.storeWait)
}

// List returns the uploads the organisation holds for the notice, oldest first.
func (s *Service) List(ctx context.Context, org, noticeID string) ([]docstore.Upload, error) {
	ctx, cancel := s.bounded(ctx)
	defer cancel()
	return s.store.List(ctx, org, noticeID)
}

// Delete removes an upload the organisation holds for the notice. An upload belongs to one notice and
// is deleted through it: the id of an upload of another notice is not found.
func (s *Service) Delete(ctx context.Context, org, noticeID string, id int64) error {
	ctx, cancel := s.bounded(ctx)
	defer cancel()
	deleted, err := s.store.Delete(ctx, org, noticeID, id)
	if err != nil {
		return fmt.Errorf("tenderdocs: delete the upload: %w", err)
	}
	if !deleted {
		return ErrNotFound
	}
	return nil
}

// PurgeExpired removes every upload that has expired, for every organisation, and returns how many.
func (s *Service) PurgeExpired(ctx context.Context) (int64, error) {
	ctx, cancel := s.bounded(ctx)
	defer cancel()
	return s.store.PurgeExpired(ctx)
}

// RunPurge removes the uploads that have expired, now and then every interval, until ctx ends. A
// failure is logged and tried again at the next tick: an expired upload that is still there is gone
// to every read already, so a purge that is late costs only disk.
func (s *Service) RunPurge(ctx context.Context, every time.Duration) {
	ticker := time.NewTicker(every)
	defer ticker.Stop()
	for {
		switch removed, err := s.PurgeExpired(ctx); {
		case err != nil:
			if ctx.Err() == nil {
				s.log.Warn().Err(err).Msg("tender documents: purge of expired uploads failed")
			}
		case removed > 0:
			s.log.Info().Int64("removed", removed).Msg("tender documents: expired uploads removed")
		}
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
		}
	}
}

// acquire takes a slot for reading an archive without waiting: a busy service says so and does not
// queue work.
func (s *Service) acquire() (release func(), err error) {
	select {
	case s.slots <- struct{}{}:
		return func() { <-s.slots }, nil
	default:
		return nil, ErrBusy
	}
}

// cleanName is the name of an archive as it is kept: valid text with no control characters, no
// bidirectional controls and no invisible ones, trimmed, and at most maxArchiveNameRunes characters.
func cleanName(name string) string {
	name = strings.ToValidUTF8(name, "�")
	name = strings.Map(func(r rune) rune {
		if unicode.IsControl(r) || textsafe.Invisible(r) {
			return -1
		}
		return r
	}, name)
	name = strings.TrimSpace(name)
	if utf8.RuneCountInString(name) > maxArchiveNameRunes {
		name = string([]rune(name)[:maxArchiveNameRunes])
	}
	return name
}
