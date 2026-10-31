package tenderdocs

import (
	"archive/zip"
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/rs/zerolog"

	"github.com/triodelab/anbud/internal/docread"
	"github.com/triodelab/anbud/internal/docstore"
	"github.com/triodelab/anbud/internal/ziputil"
)

const (
	testOrg    = "org-a"
	testUser   = "user-1"
	testNotice = "2026-100001"
)

func hashOf(b []byte) string {
	sum := sha256.Sum256(b)
	return hex.EncodeToString(sum[:])
}

// fakeStore is the tenant document store in memory. It keeps what Create is given, so a test can say
// what was stored and what was not, and it counts the calls.
type fakeStore struct {
	mu        sync.Mutex
	uploads   []docstore.Upload
	nextID    int64
	creates   []docstore.NewUpload
	calls     map[string]int
	evidence  map[string]docstore.Evidence
	evidenceN []int // the number of notices asked for in each EvidenceFor call
	purged    int64
	failWith  map[string]error // by method name
	hang      map[string]bool  // methods that wait until their context ends, by method name
	delay     time.Duration    // how long every call takes
	// held makes Counts report this many uploads, whatever was stored: for the quota tests.
	heldNotice, heldOrg int
	// vanishOnDelete makes Delete find nothing to delete, as if the upload went between a listing and it.
	vanishOnDelete bool
	// blockCreate, when set, makes Create announce itself on the channel and wait for its context to end.
	blockCreate chan struct{}
}

func newFakeStore() *fakeStore {
	return &fakeStore{calls: map[string]int{}, evidence: map[string]docstore.Evidence{}, failWith: map[string]error{}, hang: map[string]bool{}}
}

// enter counts a call and then does what the test has asked of the method: hang until its context ends,
// take a while, or fail.
func (f *fakeStore) enter(ctx context.Context, method string) error {
	f.mu.Lock()
	f.calls[method]++
	err, hang, delay := f.failWith[method], f.hang[method], f.delay
	f.mu.Unlock()
	return pause(ctx, hang, delay, err)
}

// pause waits as a test has asked: for ever, or for delay, in either case only until ctx ends.
func pause(ctx context.Context, hang bool, delay time.Duration, then error) error {
	if hang {
		<-ctx.Done()
		return ctx.Err()
	}
	if delay > 0 {
		select {
		case <-time.After(delay):
		case <-ctx.Done():
			return ctx.Err()
		}
	}
	return then
}

func (f *fakeStore) callCount(method string) int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.calls[method]
}

func (f *fakeStore) Create(ctx context.Context, in docstore.NewUpload) (docstore.Upload, bool, error) {
	if err := f.enter(ctx, "Create"); err != nil {
		return docstore.Upload{}, false, err
	}
	// The real store refuses what is too large to hold once encoded, before it asks the database.
	if _, _, err := in.Encoded(); err != nil {
		return docstore.Upload{}, false, err
	}
	if f.blockCreate != nil {
		f.blockCreate <- struct{}{} // says that Create has been reached
		<-ctx.Done()
		return docstore.Upload{}, false, ctx.Err()
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	for _, u := range f.uploads {
		if u.OrgID == in.OrgID && u.NoticeID == in.NoticeID && u.ArchiveSHA256 == in.ArchiveSHA256 {
			return u, false, nil
		}
	}
	// The real store counts here, in the transaction that stores the upload and under a lock of the organisation, so
	// that uploads made at once cannot pass the limit. The same archive was looked for first: it is not counted.
	if in.MaxPerNotice > 0 || in.MaxPerOrg > 0 {
		forNotice, forOrg := f.countLocked(in.OrgID, in.NoticeID)
		switch {
		case in.MaxPerNotice > 0 && forNotice >= in.MaxPerNotice:
			return docstore.Upload{}, false, &docstore.QuotaError{Scope: "notice"}
		case in.MaxPerOrg > 0 && forOrg >= in.MaxPerOrg:
			return docstore.Upload{}, false, &docstore.QuotaError{Scope: "organisation"}
		}
	}
	f.nextID++
	read, unread := 0, 0
	for _, d := range in.Documents {
		switch d.Status {
		case docstore.StatusRead:
			read++
		case docstore.StatusDuplicate:
		default:
			unread++
		}
	}
	now := time.Now().UTC()
	up := docstore.Upload{
		ID: f.nextID, OrgID: in.OrgID, NoticeID: in.NoticeID, UploadedBy: in.UploadedBy, ArchiveName: in.ArchiveName,
		ArchiveSHA256: in.ArchiveSHA256, ArchiveBytes: in.ArchiveBytes, Documents: in.Documents, Findings: in.Findings,
		ReadCount: read, UnreadCount: unread, FindingsDropped: in.FindingsDropped, CreatedAt: now, ExpiresAt: now.Add(in.Retention),
	}
	f.uploads = append(f.uploads, up)
	f.creates = append(f.creates, in)
	return up, true, nil
}

func (f *fakeStore) List(ctx context.Context, org, notice string) ([]docstore.Upload, error) {
	if err := f.enter(ctx, "List"); err != nil {
		return nil, err
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	var out []docstore.Upload
	for _, u := range f.uploads {
		if u.OrgID == org && u.NoticeID == notice {
			out = append(out, u)
		}
	}
	return out, nil
}

func (f *fakeStore) FindSame(ctx context.Context, org, notice, sha string) (docstore.Upload, bool, error) {
	if err := f.enter(ctx, "FindSame"); err != nil {
		return docstore.Upload{}, false, err
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	for _, u := range f.uploads {
		if u.OrgID == org && u.NoticeID == notice && u.ArchiveSHA256 == sha {
			return u, true, nil
		}
	}
	return docstore.Upload{}, false, nil
}

func (f *fakeStore) Delete(ctx context.Context, org, notice string, id int64) (bool, error) {
	if err := f.enter(ctx, "Delete"); err != nil {
		return false, err
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.vanishOnDelete {
		return false, nil
	}
	for i, u := range f.uploads {
		if u.OrgID == org && u.NoticeID == notice && u.ID == id {
			f.uploads = append(f.uploads[:i], f.uploads[i+1:]...)
			return true, nil
		}
	}
	return false, nil
}

func (f *fakeStore) Counts(ctx context.Context, org, notice string) (int, int, error) {
	if err := f.enter(ctx, "Counts"); err != nil {
		return 0, 0, err
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	forNotice, forOrg := f.countLocked(org, notice)
	return forNotice, forOrg, nil
}

// countLocked is what the organisation holds, for the notice and in all. The caller holds the lock.
func (f *fakeStore) countLocked(org, notice string) (forNotice, forOrg int) {
	forNotice, forOrg = f.heldNotice, f.heldOrg
	for _, u := range f.uploads {
		if u.OrgID == org {
			forOrg++
			if u.NoticeID == notice {
				forNotice++
			}
		}
	}
	return forNotice, forOrg
}

func (f *fakeStore) EvidenceFor(ctx context.Context, org string, notices []string) (map[string]docstore.Evidence, error) {
	if err := f.enter(ctx, "EvidenceFor"); err != nil {
		return nil, err
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	f.evidenceN = append(f.evidenceN, len(notices))
	out := map[string]docstore.Evidence{}
	for _, id := range notices {
		if ev, ok := f.evidence[id]; ok {
			out[id] = ev
		}
	}
	return out, nil
}

func (f *fakeStore) PurgeExpired(ctx context.Context) (int64, error) {
	if err := f.enter(ctx, "PurgeExpired"); err != nil {
		return 0, err
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.purged, nil
}

type fakeNotices struct {
	held  map[string]bool
	err   error
	calls int
	mu    sync.Mutex
	// hang makes HasNotice wait until its context ends; delay makes it take that long.
	hang  bool
	delay time.Duration
}

func (n *fakeNotices) HasNotice(ctx context.Context, id string) (bool, error) {
	n.mu.Lock()
	n.calls++
	held, err, hang, delay := n.held[id], n.err, n.hang, n.delay
	n.mu.Unlock()
	if err := pause(ctx, hang, delay, nil); err != nil {
		return false, err
	}
	return held, err
}

// scriptReader is a document reader a test controls: it counts what it is asked to read, and answers
// from a function.
type scriptReader struct {
	mu    sync.Mutex
	reads int
	fn    func(ctx context.Context, data []byte) (docread.Content, error)
}

func (r *scriptReader) Read(ctx context.Context, data []byte) (docread.Content, error) {
	r.mu.Lock()
	r.reads++
	fn := r.fn
	r.mu.Unlock()
	return fn(ctx, data)
}

func (r *scriptReader) count() int {
	r.mu.Lock()
	defer r.mu.Unlock()
	return r.reads
}

// testLimits are small enough to reach in a test.
func testLimits() ziputil.Limits {
	return ziputil.Limits{MaxArchiveBytes: 8 << 20, MaxEntries: 1000, MaxEntryBytes: 4 << 20, MaxTotalBytes: 16 << 20, MaxRatio: 100, MaxDepth: 3}
}

// fixture is a service over fakes, with a real document reader unless a test replaces it.
type fixture struct {
	svc     *Service
	store   *fakeStore
	notices *fakeNotices
	logs    *bytes.Buffer
}

func newFixture(t *testing.T, tweak ...func(*Options)) *fixture {
	t.Helper()
	store := newFakeStore()
	notices := &fakeNotices{held: map[string]bool{testNotice: true}}
	logs := &bytes.Buffer{}
	logger := zerolog.New(logs)
	o := Options{
		Store: store, Notices: notices,
		Reader:    docread.NewReader(docread.InProcessPDF{}, docread.DefaultLimits),
		Limits:    testLimits(),
		Retention: 90 * 24 * time.Hour,
		Logger:    &logger,
	}
	for _, f := range tweak {
		f(&o)
	}
	svc, err := New(o)
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	return &fixture{svc: svc, store: store, notices: notices, logs: logs}
}

func (f *fixture) upload(t *testing.T, archive []byte) (docstore.Upload, bool, error) {
	t.Helper()
	return f.svc.Upload(context.Background(), UploadRequest{
		OrgID: testOrg, UserID: testUser, NoticeID: testNotice, ArchiveName: "konkurransegrunnlag.zip", Archive: archive,
	})
}

// entry is one file of a test archive.
type entry struct {
	name string
	data []byte
}

func file(name, data string) entry { return entry{name: name, data: []byte(data)} }

// zipOf writes an archive.
func zipOf(t *testing.T, entries ...entry) []byte {
	t.Helper()
	var buf bytes.Buffer
	w := zip.NewWriter(&buf)
	for _, e := range entries {
		fw, err := w.CreateHeader(&zip.FileHeader{Name: e.name, Method: zip.Deflate})
		if err != nil {
			t.Fatalf("create %q: %v", e.name, err)
		}
		if _, err := fw.Write(e.data); err != nil {
			t.Fatalf("write %q: %v", e.name, err)
		}
	}
	if err := w.Close(); err != nil {
		t.Fatalf("close: %v", err)
	}
	return buf.Bytes()
}

// docx builds a DOCX with one paragraph for each string.
func docx(t *testing.T, paragraphs ...string) []byte {
	t.Helper()
	var body strings.Builder
	for _, p := range paragraphs {
		body.WriteString(`<w:p><w:r><w:t xml:space="preserve">` + p + `</w:t></w:r></w:p>`)
	}
	return zipOf(t,
		file("[Content_Types].xml", `<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>`),
		file("_rels/.rels", `<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>`),
		file("word/document.xml", `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>`+body.String()+`</w:body></w:document>`),
	)
}

// miniPDF writes a PDF with one page for each string. A page whose text is empty has no text at all,
// like a scanned page that is only a picture.
func miniPDF(pages ...string) []byte {
	var b bytes.Buffer
	var offsets []int
	obj := func(body string) {
		offsets = append(offsets, b.Len())
		fmt.Fprintf(&b, "%d 0 obj\n%s\nendobj\n", len(offsets), body)
	}
	b.WriteString("%PDF-1.4\n")
	obj("<< /Type /Catalog /Pages 2 0 R >>")
	kids := make([]string, len(pages))
	for i := range pages {
		kids[i] = fmt.Sprintf("%d 0 R", 4+2*i)
	}
	obj(fmt.Sprintf("<< /Type /Pages /Kids [%s] /Count %d >>", strings.Join(kids, " "), len(pages)))
	obj("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>")
	for i, text := range pages {
		contentNo := 5 + 2*i
		obj(fmt.Sprintf("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 3 0 R >> >> /Contents %d 0 R >>", contentNo))
		stream := ""
		if text != "" {
			stream = "BT /F1 12 Tf 50 780 Td (" + winAnsi(text) + ") Tj ET"
		}
		obj(fmt.Sprintf("<< /Length %d >>\nstream\n%s\nendstream", len(stream), stream))
	}
	xref := b.Len()
	fmt.Fprintf(&b, "xref\n0 %d\n0000000000 65535 f \n", len(offsets)+1)
	for _, off := range offsets {
		fmt.Fprintf(&b, "%010d 00000 n \n", off)
	}
	fmt.Fprintf(&b, "trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n", len(offsets)+1, xref)
	return b.Bytes()
}

func winAnsi(s string) string {
	r := strings.NewReplacer(`\`, `\\`, `(`, `\(`, `)`, `\)`, "æ", "\xe6", "ø", "\xf8", "å", "\xe5", "Æ", "\xc6", "Ø", "\xd8", "Å", "\xc5")
	return r.Replace(s)
}

// byPath indexes the manifest of an upload.
func byPath(docs []docstore.Document) map[string]docstore.Document {
	out := map[string]docstore.Document{}
	for _, d := range docs {
		out[d.Path] = d
	}
	return out
}

func capabilities(fs []docstore.Finding) map[string][]docstore.Finding {
	out := map[string][]docstore.Finding{}
	for _, f := range fs {
		out[f.Capability] = append(out[f.Capability], f)
	}
	return out
}

// readsAs makes a reader answer that every file holds n paragraphs of text that names nothing.
func readsAs(n int) func(context.Context, []byte) (docread.Content, error) {
	return func(context.Context, []byte) (docread.Content, error) {
		c := docread.Content{Kind: docread.KindText, Extractor: "text", Chars: n}
		for i := 1; i <= n; i++ {
			c.Units = append(c.Units, docread.Unit{Paragraph: i, Text: "x"})
		}
		return c, nil
	}
}
