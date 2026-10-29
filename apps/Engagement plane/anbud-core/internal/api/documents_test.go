package api

import (
	"bufio"
	"bytes"
	"compress/gzip"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/rs/zerolog"

	"github.com/triodelab/anbud/internal/docstore"
	"github.com/triodelab/anbud/internal/tenderdocs"
)

const (
	docsMax    = 4 << 20
	docsNotice = "2026-100001"
	docsPath   = "/api/v1/notices/" + docsNotice + "/documents"
)

// fakeDocuments is the tender document service, scripted by a test and recording what it was asked.
type fakeDocuments struct {
	mu       sync.Mutex
	uploads  []tenderdocs.UploadRequest
	upload   func(tenderdocs.UploadRequest) (docstore.Upload, bool, error)
	list     func(org, notice string) ([]docstore.Upload, error)
	del      func(org, notice string, id int64) error
	deletes  [][3]string
	listings [][2]string
}

func (f *fakeDocuments) MaxArchiveBytes() int64 { return docsMax }

func (f *fakeDocuments) Upload(_ context.Context, r tenderdocs.UploadRequest) (docstore.Upload, bool, error) {
	f.mu.Lock()
	// The body is the server's buffer, and it is only valid during the call: keep a copy.
	r.Archive = append([]byte(nil), r.Archive...)
	f.uploads = append(f.uploads, r)
	fn := f.upload
	f.mu.Unlock()
	if fn == nil {
		return sampleUpload(), true, nil
	}
	return fn(r)
}

func (f *fakeDocuments) List(_ context.Context, org, notice string) ([]docstore.Upload, error) {
	f.mu.Lock()
	f.listings = append(f.listings, [2]string{org, notice})
	fn := f.list
	f.mu.Unlock()
	if fn == nil {
		return nil, nil
	}
	return fn(org, notice)
}

func (f *fakeDocuments) Delete(_ context.Context, org, notice string, id int64) error {
	f.mu.Lock()
	f.deletes = append(f.deletes, [3]string{org, notice, fmt.Sprint(id)})
	fn := f.del
	f.mu.Unlock()
	if fn == nil {
		return nil
	}
	return fn(org, notice, id)
}

func (f *fakeDocuments) uploadCount() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return len(f.uploads)
}

func sampleUpload() docstore.Upload {
	at := time.Date(2026, time.October, 10, 9, 0, 0, 0, time.UTC)
	return docstore.Upload{
		ID: 7, OrgID: "org-a", NoticeID: docsNotice, UploadedBy: "user-1", ArchiveName: "konkurransegrunnlag.zip",
		ArchiveSHA256: strings.Repeat("a", 64), ArchiveBytes: 1234,
		Documents: []docstore.Document{{Path: "kg.pdf", Kind: "pdf", Size: 900, SHA256: strings.Repeat("b", 64), Status: docstore.StatusRead, Pages: 3, Chars: 400, Extractor: "pdf"}},
		Findings:  []docstore.Finding{{Document: "kg.pdf", Capability: "iso9001", Class: "requirement", Page: 2, Snippet: "Leverandøren skal ha ISO 9001."}},
		ReadCount: 1, UnreadCount: 0, FindingsDropped: 4, CreatedAt: at, ExpiresAt: at.Add(90 * 24 * time.Hour),
	}
}

func docsServer(t *testing.T, docs *fakeDocuments) (*fiber.App, *logBuffer) {
	t.Helper()
	logs := &logBuffer{}
	logger := zerolog.New(logs)
	return newServer(ServerConfig{Documents: docs, Logger: &logger}), logs
}

// tenant is the headers of an authenticated request for an organisation.
func tenant(extra ...string) map[string]string {
	h := map[string]string{"X-API-Key": serverKey, "X-Org-ID": "org-a", "X-User-ID": "user-1", "Content-Type": "application/zip"}
	for i := 0; i+1 < len(extra); i += 2 {
		if extra[i+1] == "" {
			delete(h, extra[i])
		} else {
			h[extra[i]] = extra[i+1]
		}
	}
	return h
}

func send(t *testing.T, app *fiber.App, method, path string, body []byte, headers map[string]string) (int, string, http.Header) {
	t.Helper()
	req := httptest.NewRequest(method, path, bytes.NewReader(body))
	for k, v := range headers {
		req.Header.Set(k, v)
	}
	resp, err := app.Test(req, 30000)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	defer resp.Body.Close()
	raw, _ := io.ReadAll(resp.Body)
	return resp.StatusCode, string(raw), resp.Header
}

// answerToHeadersOnly sends the headers of a request that declares a body of the given length, and
// no body, to a server that is really listening, and returns what it answers. app.Test cannot show
// this: it reports a body the server refuses as an error, and a client that sends the whole body
// races the server closing the connection. A server that decides from the headers alone answers at
// once, and that is what is under test.
func answerToHeadersOnly(t *testing.T, app *fiber.App, method, path string, headers map[string]string, contentLength int) (int, string) {
	t.Helper()
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("listen: %v", err)
	}
	go func() { _ = app.Listener(ln) }()
	t.Cleanup(func() { _ = app.Shutdown() })

	conn, err := net.DialTimeout("tcp", ln.Addr().String(), 5*time.Second)
	if err != nil {
		t.Fatalf("dial: %v", err)
	}
	defer conn.Close()
	var req strings.Builder
	fmt.Fprintf(&req, "%s %s HTTP/1.1\r\nHost: anbud\r\nContent-Length: %d\r\n", method, path, contentLength)
	for k, v := range headers {
		fmt.Fprintf(&req, "%s: %s\r\n", k, v)
	}
	req.WriteString("\r\n")
	if _, err := io.WriteString(conn, req.String()); err != nil {
		t.Fatalf("write: %v", err)
	}
	_ = conn.SetReadDeadline(time.Now().Add(5 * time.Second))
	resp, err := http.ReadResponse(bufio.NewReader(conn), nil)
	if err != nil {
		t.Fatalf("no answer to the headers alone: %v", err)
	}
	defer resp.Body.Close()
	raw, _ := io.ReadAll(resp.Body)
	return resp.StatusCode, string(raw)
}

type errorEnvelope struct {
	Error struct {
		Code    string          `json:"code"`
		Message string          `json:"message"`
		Details json.RawMessage `json:"details"`
	} `json:"error"`
}

func decodeError(t *testing.T, raw string) errorEnvelope {
	t.Helper()
	var e errorEnvelope
	if err := json.Unmarshal([]byte(raw), &e); err != nil || e.Error.Code == "" || e.Error.Message == "" {
		t.Fatalf("not an error envelope: %q (%v)", raw, err)
	}
	return e
}

func TestDocumentRoutesAreMountedOnlyWhenTheServiceIsConfigured(t *testing.T) {
	t.Parallel()

	app := newServer(ServerConfig{})
	for _, m := range []string{http.MethodPost, http.MethodGet, http.MethodDelete} {
		path := docsPath
		if m == http.MethodDelete {
			path += "/7"
		}
		status, raw, _ := send(t, app, m, path, []byte("x"), tenant())
		if status != 404 || decodeError(t, raw).Error.Code != "not_found" {
			t.Errorf("%s without the service: %d %s", m, status, raw)
		}
	}
}

func TestDocumentRoutesNeedTheKeyAndAnOrganisation(t *testing.T) {
	t.Parallel()

	docs := &fakeDocuments{}
	app, _ := docsServer(t, docs)
	for _, m := range []string{http.MethodPost, http.MethodGet, http.MethodDelete} {
		path := docsPath
		if m == http.MethodDelete {
			path += "/7"
		}
		if status, _, _ := send(t, app, m, path, []byte("x"), tenant("X-API-Key", "wrong-key")); status != 401 {
			t.Errorf("%s with a wrong key: %d", m, status)
		}
		status, raw, _ := send(t, app, m, path, []byte("x"), tenant("X-Org-ID", ""))
		if status != 400 || decodeError(t, raw).Error.Code != "organization_required" {
			t.Errorf("%s without an organisation: %d %s", m, status, raw)
		}
	}
	if docs.uploadCount() != 0 || len(docs.listings) != 0 || len(docs.deletes) != 0 {
		t.Errorf("the service was reached by requests that should have been refused")
	}
}

func TestUploadHandsTheArchiveAndWhoSentItToTheService(t *testing.T) {
	t.Parallel()

	docs := &fakeDocuments{}
	app, _ := docsServer(t, docs)
	archive := []byte("PK\x03\x04 pretend this is a zip archive")
	status, raw, _ := send(t, app, http.MethodPost, docsPath, archive,
		tenant("X-Archive-Name", "Konkurransegrunnlag%20%C3%B8st.zip"))
	if status != 201 {
		t.Fatalf("status = %d %s", status, raw)
	}
	if len(docs.uploads) != 1 {
		t.Fatalf("uploads = %d", len(docs.uploads))
	}
	got := docs.uploads[0]
	if got.OrgID != "org-a" || got.UserID != "user-1" || got.NoticeID != docsNotice || got.ArchiveName != "Konkurransegrunnlag øst.zip" ||
		got.ZDR || !bytes.Equal(got.Archive, archive) {
		t.Errorf("service was given %+v", got)
	}

	var envelope struct {
		Data map[string]any `json:"data"`
	}
	if err := json.Unmarshal([]byte(raw), &envelope); err != nil {
		t.Fatalf("not JSON: %v", err)
	}
	d := envelope.Data
	for field, want := range map[string]any{
		"id": float64(7), "notice_id": docsNotice, "archive_name": "konkurransegrunnlag.zip", "archive_bytes": float64(1234),
		"read": float64(1), "unread": float64(0), "findings_dropped": float64(4), "uploaded_by": "user-1",
		"created_at": "2026-10-10T09:00:00Z", "expires_at": "2027-01-08T09:00:00Z",
	} {
		if d[field] != want {
			t.Errorf("data.%s = %v, want %v", field, d[field], want)
		}
	}
	if docs, _ := d["documents"].([]any); len(docs) != 1 {
		t.Errorf("data.documents = %v", d["documents"])
	}
	if fs, _ := d["findings"].([]any); len(fs) != 1 {
		t.Errorf("data.findings = %v", d["findings"])
	}
}

// The same archive again is the upload that exists: 200, not 201.
func TestUploadOfAnArchiveThatIsAlreadyStoredIsNotCreated(t *testing.T) {
	t.Parallel()

	docs := &fakeDocuments{upload: func(tenderdocs.UploadRequest) (docstore.Upload, bool, error) { return sampleUpload(), false, nil }}
	app, _ := docsServer(t, docs)
	if status, raw, _ := send(t, app, http.MethodPost, docsPath, []byte("x"), tenant()); status != 200 {
		t.Errorf("status = %d %s, want 200", status, raw)
	}
}

func TestUploadAcceptsTheContentTypesOfAnArchiveAndNothingElse(t *testing.T) {
	t.Parallel()

	for contentType, want := range map[string]int{
		"application/zip":                 201,
		"application/x-zip-compressed":    201,
		"application/octet-stream":        201,
		"Application/ZIP; charset=binary": 201,
		"multipart/form-data; boundary=x": 415,
		"application/json":                415,
		"text/plain":                      415,
		"":                                415,
		"not a media type":                415,
	} {
		docs := &fakeDocuments{}
		app, _ := docsServer(t, docs)
		body := []byte("x")
		if strings.HasPrefix(contentType, "multipart/") {
			body = []byte("--x\r\nContent-Disposition: form-data; name=\"archive\"; filename=\"a.zip\"\r\n\r\nPK\r\n--x--\r\n")
		}
		status, raw, _ := send(t, app, http.MethodPost, docsPath, body, tenant("Content-Type", contentType))
		if status != want {
			t.Errorf("%q: status %d %s, want %d", contentType, status, raw, want)
		}
		if want == 415 {
			if decodeError(t, raw).Error.Code != "unsupported_media_type" {
				t.Errorf("%q: %s", contentType, raw)
			}
			if docs.uploadCount() != 0 {
				t.Errorf("%q: the service was reached", contentType)
			}
		}
	}
}

func TestUploadRefusesAnArchiveNameThatIsNotText(t *testing.T) {
	t.Parallel()

	docs := &fakeDocuments{}
	app, _ := docsServer(t, docs)
	status, raw, _ := send(t, app, http.MethodPost, docsPath, []byte("x"), tenant("X-Archive-Name", "%zz%"))
	e := decodeError(t, raw)
	if status != 400 || e.Error.Code != "validation_failed" || !strings.Contains(string(e.Error.Details), "X-Archive-Name") {
		t.Errorf("%d %s", status, raw)
	}
	if strings.Contains(raw, "%zz") {
		t.Errorf("the refusal quotes the value: %s", raw)
	}
	if docs.uploadCount() != 0 {
		t.Errorf("the service was reached")
	}
}

func TestUploadPassesTheZeroDataRetentionHeaderOn(t *testing.T) {
	t.Parallel()

	for value, want := range map[string]bool{"true": true, "TRUE": true, " true ": true, "1": true, "false": false, "0": false, "yes": false, "": false} {
		docs := &fakeDocuments{}
		app, _ := docsServer(t, docs)
		send(t, app, http.MethodPost, docsPath, []byte("x"), tenant("X-ZDR", value))
		if len(docs.uploads) != 1 || docs.uploads[0].ZDR != want {
			t.Errorf("X-ZDR %q: ZDR = %v, want %v", value, docs.uploads, want)
		}
	}
}

func TestUploadMapsWhatTheServiceRefusesToAStatusAndACode(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name       string
		err        error
		wantStatus int
		wantCode   string
		wantDetail string
	}{
		{"zero data retention", tenderdocs.ErrZDR, 409, "zdr_persistence_forbidden", ""},
		{"a notice id that is not one", tenderdocs.ErrInvalidNotice, 400, "bad_request", ""},
		{"a notice that is not held", tenderdocs.ErrNoticeNotFound, 404, "not_found", ""},
		{"the slots are taken", tenderdocs.ErrBusy, 429, "rate_limited", ""},
		{"too slow", tenderdocs.ErrTimeout, 504, "timeout", ""},
		{"quota of the notice", &tenderdocs.QuotaError{Scope: "notice"}, 409, "limit_reached", `"scope":"notice"`},
		{"quota of the organisation", &tenderdocs.QuotaError{Scope: "organisation"}, 409, "limit_reached", `"scope":"organisation"`},
		{"an empty body", &tenderdocs.ArchiveError{Reason: tenderdocs.ReasonEmpty}, 400, "bad_request", ""},
		{"an archive that is too large", &tenderdocs.ArchiveError{Reason: tenderdocs.ReasonTooLarge}, 413, "payload_too_large", ""},
		{"not a zip", &tenderdocs.ArchiveError{Reason: tenderdocs.ReasonNotAZip}, 422, "invalid_archive", `"reason":"not_a_zip"`},
		{"too many entries", &tenderdocs.ArchiveError{Reason: tenderdocs.ReasonTooManyEntries}, 422, "invalid_archive", `"reason":"too_many_entries"`},
		{"too many files", &tenderdocs.ArchiveError{Reason: tenderdocs.ReasonTooManyFiles}, 422, "invalid_archive", `"reason":"too_many_files"`},
		{"a zip bomb", &tenderdocs.ArchiveError{Reason: tenderdocs.ReasonExpandsTooFar}, 422, "invalid_archive", `"reason":"expands_too_far"`},
		{"an organisation the store refuses", fmt.Errorf("list: %w", docstore.ErrInvalid), 400, "bad_request", ""},
		{"a wrapped error", fmt.Errorf("tenderdocs: store: %w", tenderdocs.ErrBusy), 429, "rate_limited", ""},
	}
	for _, tc := range tests {
		docs := &fakeDocuments{upload: func(tenderdocs.UploadRequest) (docstore.Upload, bool, error) { return docstore.Upload{}, false, tc.err }}
		app, _ := docsServer(t, docs)
		status, raw, header := send(t, app, http.MethodPost, docsPath, []byte("x"), tenant())
		e := decodeError(t, raw)
		if status != tc.wantStatus || e.Error.Code != tc.wantCode {
			t.Errorf("%s: %d %s, want %d %s", tc.name, status, raw, tc.wantStatus, tc.wantCode)
		}
		if tc.wantDetail != "" && !strings.Contains(string(e.Error.Details), tc.wantDetail) {
			t.Errorf("%s: details %s, want %s", tc.name, e.Error.Details, tc.wantDetail)
		}
		if tc.wantStatus == 429 && header.Get("Retry-After") == "" {
			t.Errorf("%s: no Retry-After", tc.name)
		}
	}
}

// What the service does not know is a 500 that says nothing of its cause: the cause is in the log.
func TestUploadAnswersAnUnknownFailureWithAnInternalErrorAndLogsTheCause(t *testing.T) {
	t.Parallel()

	docs := &fakeDocuments{upload: func(tenderdocs.UploadRequest) (docstore.Upload, bool, error) {
		return docstore.Upload{}, false, errors.New("connection to db-host-7 refused for user anbud_app")
	}}
	app, logs := docsServer(t, docs)
	status, raw, _ := send(t, app, http.MethodPost, docsPath, []byte("x"), tenant())
	if status != 500 || decodeError(t, raw).Error.Code != "internal" {
		t.Fatalf("%d %s", status, raw)
	}
	if strings.Contains(raw, "db-host-7") || strings.Contains(raw, "anbud_app") {
		t.Errorf("the response leaks the cause: %s", raw)
	}
	if !strings.Contains(logs.String(), "db-host-7") {
		t.Errorf("the cause is not in the log: %s", logs.String())
	}
}

// The body limit of the server is 1 MiB, which an archive is larger than. The header hook lifts it for
// an upload that carries the key, and for nothing else.
func TestAnAuthenticatedUploadMayBeLargerThanTheOrdinaryBodyLimit(t *testing.T) {
	t.Parallel()

	docs := &fakeDocuments{}
	app, _ := docsServer(t, docs)
	body := bytes.Repeat([]byte("a"), 2<<20)
	status, raw, _ := send(t, app, http.MethodPost, docsPath, body, tenant())
	if status != 201 || docs.uploadCount() != 1 || len(docs.uploads[0].Archive) != len(body) {
		t.Fatalf("a 2 MiB upload: %d %s (%d uploads)", status, raw, docs.uploadCount())
	}
	// A trailing slash and a query string are the same route.
	for _, path := range []string{docsPath + "/", docsPath + "?x=1"} {
		if status, raw, _ := send(t, app, http.MethodPost, path, body, tenant()); status != 201 {
			t.Errorf("%s: %d %s", path, status, raw)
		}
	}
}

// The server decides how large a body it takes from the headers, before it reads any of it: a request
// that is not an upload by the gateway is refused at once, whatever it goes on to send.
func TestOnlyAnUploadThatCarriesTheKeyIsGivenALargeBody(t *testing.T) {
	t.Parallel()

	for name, tc := range map[string]struct {
		method, path string
		headers      map[string]string
	}{
		"no key":                  {http.MethodPost, docsPath, tenant("X-API-Key", "")},
		"a wrong key":             {http.MethodPost, docsPath, tenant("X-API-Key", "not-the-key")},
		"another route":           {http.MethodPost, "/api/v1/demo/company", tenant()},
		"another notice route":    {http.MethodPost, "/api/v1/notices/" + docsNotice + "/other", tenant()},
		"a deeper path":           {http.MethodPost, docsPath + "/7", tenant()},
		"another method":          {http.MethodPut, docsPath, tenant()},
		"a path that only starts": {http.MethodPost, "/api/v1/notices/documents", tenant()},
		"no notice id":            {http.MethodPost, "/api/v1/notices//documents", tenant()},
		// The hook reads the target as it was sent and the router reads the path, which ends where a fragment
		// begins: a target that the hook took for the upload route was another route to the router, and got the
		// room of an archive all the same. A client never sends a fragment, so none is taken for a path.
		"a fragment that hides the route from the router": {http.MethodPost, "/api/v1/notices/" + docsNotice + "#/documents", tenant()},
		"a fragment after the route":                      {http.MethodPost, docsPath + "#fragment", tenant()},
		"a fragment before a query":                       {http.MethodPost, "/api/v1/notices/" + docsNotice + "#/documents?x=1", tenant()},
	} {
		docs := &fakeDocuments{}
		app, _ := docsServer(t, docs)
		status, raw := answerToHeadersOnly(t, app, tc.method, tc.path, tc.headers, 2<<20)
		if status != 413 || decodeError(t, raw).Error.Code != "payload_too_large" {
			t.Errorf("%s: %d %s, want 413 payload_too_large", name, status, raw)
		}
		if docs.uploadCount() != 0 {
			t.Errorf("%s: the service was reached", name)
		}
	}
}

func TestAnUploadLargerThanTheArchiveLimitIsRefusedByTheServer(t *testing.T) {
	t.Parallel()

	docs := &fakeDocuments{}
	app, _ := docsServer(t, docs)
	status, raw := answerToHeadersOnly(t, app, http.MethodPost, docsPath, tenant(), docsMax+1)
	if status != 413 || decodeError(t, raw).Error.Code != "payload_too_large" {
		t.Errorf("%d %s", status, raw)
	}
	if docs.uploadCount() != 0 {
		t.Errorf("the service was reached")
	}
	// Exactly the limit is allowed.
	if status, raw, _ := send(t, app, http.MethodPost, docsPath, bytes.Repeat([]byte("a"), docsMax), tenant()); status != 201 {
		t.Errorf("an archive of exactly the limit: %d %s", status, raw)
	}
}

// Context.Body inflates a compressed body without a limit, so a small request could become a large
// allocation past every check made after it. No route takes one.
func TestACompressedRequestBodyIsRefusedOnEveryRoute(t *testing.T) {
	t.Parallel()

	var packed bytes.Buffer
	zw := gzip.NewWriter(&packed)
	_, _ = zw.Write(bytes.Repeat([]byte("0"), 8<<20)) // eight megabytes that pack into a few kilobytes
	_ = zw.Close()
	if packed.Len() > 64<<10 {
		t.Fatalf("the packed body is %d bytes", packed.Len())
	}

	docs := &fakeDocuments{}
	app, _ := docsServer(t, docs)
	for _, path := range []string{docsPath, "/api/v1/demo/company", "/api/v1/sources"} {
		for _, encoding := range []string{"gzip", "deflate", "br", "gzip, br", "identity, gzip"} {
			status, raw, _ := send(t, app, http.MethodPost, path, packed.Bytes(), tenant("Content-Encoding", encoding))
			if status != 415 || decodeError(t, raw).Error.Code != "unsupported_media_type" {
				t.Errorf("%s with %s: %d %s", path, encoding, status, raw)
			}
		}
	}
	if docs.uploadCount() != 0 {
		t.Errorf("the service was reached")
	}
	// "identity" means not encoded, and an empty header is no header.
	for _, encoding := range []string{"identity", "Identity", " identity "} {
		if status, raw, _ := send(t, app, http.MethodPost, docsPath, []byte("x"), tenant("Content-Encoding", encoding)); status != 201 {
			t.Errorf("Content-Encoding %q: %d %s", encoding, status, raw)
		}
	}
}

// Times are shown in UTC whatever zone the store hands them over in.
func TestTimesAreShownInUTC(t *testing.T) {
	t.Parallel()

	up := sampleUpload()
	zone := time.FixedZone("CEST", 2*3600)
	up.CreatedAt, up.ExpiresAt = up.CreatedAt.In(zone), up.ExpiresAt.In(zone)
	docs := &fakeDocuments{upload: func(tenderdocs.UploadRequest) (docstore.Upload, bool, error) { return up, true, nil }}
	app, _ := docsServer(t, docs)
	_, raw, _ := send(t, app, http.MethodPost, docsPath, []byte("x"), tenant())
	if !strings.Contains(raw, `"created_at":"2026-10-10T09:00:00Z"`) || !strings.Contains(raw, `"expires_at":"2027-01-08T09:00:00Z"`) {
		t.Errorf("%s", raw)
	}
}

func TestListReturnsTheUploadsOfTheNotice(t *testing.T) {
	t.Parallel()

	docs := &fakeDocuments{list: func(org, notice string) ([]docstore.Upload, error) {
		return []docstore.Upload{sampleUpload()}, nil
	}}
	app, _ := docsServer(t, docs)
	status, raw, _ := send(t, app, http.MethodGet, docsPath, nil, tenant("Content-Type", ""))
	if status != 200 {
		t.Fatalf("%d %s", status, raw)
	}
	var got struct {
		Data struct {
			Uploads []map[string]any `json:"uploads"`
		} `json:"data"`
	}
	if err := json.Unmarshal([]byte(raw), &got); err != nil || len(got.Data.Uploads) != 1 || got.Data.Uploads[0]["id"] != float64(7) {
		t.Errorf("%s (%v)", raw, err)
	}
	if len(docs.listings) != 1 || docs.listings[0] != [2]string{"org-a", docsNotice} {
		t.Errorf("the service was asked %v", docs.listings)
	}
}

func TestListOfANoticeWithNoUploadsIsAnEmptyListNotNull(t *testing.T) {
	t.Parallel()

	app, _ := docsServer(t, &fakeDocuments{})
	_, raw, _ := send(t, app, http.MethodGet, docsPath, nil, tenant("Content-Type", ""))
	if !strings.Contains(raw, `"uploads":[]`) {
		t.Errorf("%s", raw)
	}
}

func TestAnUploadWithNothingListedIsAListNotNull(t *testing.T) {
	t.Parallel()

	empty := sampleUpload()
	empty.Documents, empty.Findings = nil, nil
	docs := &fakeDocuments{upload: func(tenderdocs.UploadRequest) (docstore.Upload, bool, error) { return empty, true, nil }}
	app, _ := docsServer(t, docs)
	_, raw, _ := send(t, app, http.MethodPost, docsPath, []byte("x"), tenant())
	if !strings.Contains(raw, `"documents":[]`) || !strings.Contains(raw, `"findings":[]`) {
		t.Errorf("%s", raw)
	}
}

func TestListMapsWhatTheServiceRefuses(t *testing.T) {
	t.Parallel()

	docs := &fakeDocuments{list: func(string, string) ([]docstore.Upload, error) { return nil, docstore.ErrInvalid }}
	app, _ := docsServer(t, docs)
	status, raw, _ := send(t, app, http.MethodGet, "/api/v1/notices/not-a-notice/documents", nil, tenant("Content-Type", ""))
	if status != 400 || decodeError(t, raw).Error.Code != "bad_request" {
		t.Errorf("%d %s", status, raw)
	}
}

func TestDeleteRemovesAnUploadAndAnswersNoContent(t *testing.T) {
	t.Parallel()

	docs := &fakeDocuments{}
	app, _ := docsServer(t, docs)
	status, raw, _ := send(t, app, http.MethodDelete, docsPath+"/7", nil, tenant("Content-Type", ""))
	if status != 204 || raw != "" {
		t.Errorf("%d %q", status, raw)
	}
	if len(docs.deletes) != 1 || docs.deletes[0] != [3]string{"org-a", docsNotice, "7"} {
		t.Errorf("the service was asked %v", docs.deletes)
	}
}

func TestDeleteOfAnUploadThatIsNotThereIsNotFound(t *testing.T) {
	t.Parallel()

	docs := &fakeDocuments{del: func(string, string, int64) error { return tenderdocs.ErrNotFound }}
	app, _ := docsServer(t, docs)
	status, raw, _ := send(t, app, http.MethodDelete, docsPath+"/7", nil, tenant("Content-Type", ""))
	if status != 404 || decodeError(t, raw).Error.Code != "not_found" {
		t.Errorf("%d %s", status, raw)
	}
}

func TestDeleteRefusesAnIdThatIsNotAnUploadId(t *testing.T) {
	t.Parallel()

	docs := &fakeDocuments{}
	app, _ := docsServer(t, docs)
	for _, id := range []string{"abc", "0", "-1", "1.5", "99999999999999999999", "7;8", "%20"} {
		status, raw, _ := send(t, app, http.MethodDelete, docsPath+"/"+id, nil, tenant("Content-Type", ""))
		if status != 400 || decodeError(t, raw).Error.Code != "bad_request" {
			t.Errorf("id %q: %d %s", id, status, raw)
		}
	}
	if len(docs.deletes) != 0 {
		t.Errorf("the service was asked to delete %v", docs.deletes)
	}
}

// The server parses a multipart body before a handler sees it, unless it is told not to, and parses a
// large file into a temporary file on disk. Nothing here takes a form, and an archive is not written
// anywhere: a multipart body, whatever it holds, is refused by its content type and never parsed.
func TestAMultipartBodyIsRefusedByItsContentTypeAndNeverParsed(t *testing.T) {
	t.Parallel()

	docs := &fakeDocuments{}
	app, _ := docsServer(t, docs)
	// A body that is not a multipart form at all, under the content type of one: a parser would fail.
	status, raw, _ := send(t, app, http.MethodPost, docsPath, []byte("this is not a form"), tenant("Content-Type", "multipart/form-data; boundary=x"))
	if status != 415 || decodeError(t, raw).Error.Code != "unsupported_media_type" {
		t.Errorf("%d %s", status, raw)
	}
	if docs.uploadCount() != 0 {
		t.Errorf("the service was reached")
	}
}

// The framework inflates by the last Content-Encoding line, so every line must be looked at: a request
// that says "identity" and then "gzip" is a compressed one.
func TestACompressedBodyIsRefusedWhicheverLineSaysSo(t *testing.T) {
	t.Parallel()

	docs := &fakeDocuments{}
	app, _ := docsServer(t, docs)
	body := []byte("x")
	for name, lines := range map[string][]string{
		"identity and then gzip": {"identity", "gzip"},
		"gzip and then identity": {"gzip", "identity"},
		"empty and then gzip":    {"", "gzip"},
		"three lines":            {"identity", "identity", "br"},
	} {
		for _, path := range []string{docsPath, "/api/v1/demo/company"} {
			req := httptest.NewRequest(http.MethodPost, path, bytes.NewReader(body))
			req.Header.Set("X-API-Key", serverKey)
			req.Header.Set("X-Org-ID", "org-a")
			req.Header.Set("Content-Type", "application/zip")
			for _, v := range lines {
				req.Header.Add("Content-Encoding", v)
			}
			resp, err := app.Test(req, 10000)
			if err != nil {
				t.Fatalf("app.Test: %v", err)
			}
			raw, _ := io.ReadAll(resp.Body)
			resp.Body.Close()
			if resp.StatusCode != 415 || decodeError(t, string(raw)).Error.Code != "unsupported_media_type" {
				t.Errorf("%s on %s: %d %s, want 415", name, path, resp.StatusCode, raw)
			}
		}
	}
	if docs.uploadCount() != 0 {
		t.Errorf("the service was reached")
	}
}

// The name is text in UTF-8, and what is not text is refused and not quietly changed.
func TestAnArchiveNameThatIsNotUTF8IsRefused(t *testing.T) {
	t.Parallel()

	docs := &fakeDocuments{}
	app, _ := docsServer(t, docs)
	for name, value := range map[string]string{"a lone byte": "%FF", "two bytes": "a%C3b", "a surrogate": "%ED%A0%80"} {
		status, raw, _ := send(t, app, http.MethodPost, docsPath, []byte("x"), tenant("X-Archive-Name", value))
		if status != 400 || decodeError(t, raw).Error.Code != "validation_failed" {
			t.Errorf("%s: %d %s", name, status, raw)
		}
	}
	if docs.uploadCount() != 0 {
		t.Errorf("the service was reached")
	}
	if status, _, _ := send(t, app, http.MethodPost, docsPath, []byte("x"), tenant("X-Archive-Name", "%C3%A6%C3%B8%C3%A5.zip")); status != 201 {
		t.Errorf("a name in Norwegian letters: %d", status)
	}
	if got := docs.uploads[len(docs.uploads)-1].ArchiveName; got != "æøå.zip" {
		t.Errorf("the name reached the service as %q", got)
	}
}

// The handler reads the body as it came and inflates nothing: whatever lets a compressed body by, the
// service is given the bytes that were sent. The handler is mounted bare here, with nothing in front of it
// that would refuse the encoding.
func TestTheUploadHandlerReadsTheBodyRawAndInflatesNothing(t *testing.T) {
	t.Parallel()

	docs := &fakeDocuments{}
	app := fiber.New()
	app.Post("/upload", uploadHandler(docs))

	var packed bytes.Buffer
	zw := gzip.NewWriter(&packed)
	if _, err := zw.Write([]byte(strings.Repeat("inflated ", 1000))); err != nil {
		t.Fatal(err)
	}
	if err := zw.Close(); err != nil {
		t.Fatal(err)
	}
	req := httptest.NewRequest(http.MethodPost, "/upload", bytes.NewReader(packed.Bytes()))
	req.Header.Set("Content-Type", "application/zip")
	req.Header.Set("Content-Encoding", "gzip")
	resp, err := app.Test(req, 10000)
	if err != nil {
		t.Fatal(err)
	}
	resp.Body.Close()
	if docs.uploadCount() != 1 {
		t.Fatalf("the service was reached %d times, status %d", docs.uploadCount(), resp.StatusCode)
	}
	if got := docs.uploads[0].Archive; !bytes.Equal(got, packed.Bytes()) {
		t.Errorf("the service was given %d bytes, want the %d that were sent: the body was inflated", len(got), packed.Len())
	}
}

// Zero data retention is a posture that is only ever left out when no line of the header asks for it: of two
// lines the server reads the first, and which of them is the gateway's is not known. The service refuses what is
// marked (tenderdocs.ErrZDR), so what matters here is what the request hands it.
func TestAnyLineOfTheZeroDataRetentionHeaderThatSaysSoMarksTheUpload(t *testing.T) {
	t.Parallel()

	docs := &fakeDocuments{}
	app, _ := docsServer(t, docs)
	zdr := func(lines ...string) bool {
		req := httptest.NewRequest(http.MethodPost, docsPath, bytes.NewReader([]byte("x")))
		for k, v := range tenant() {
			req.Header.Set(k, v)
		}
		for _, v := range lines {
			req.Header.Add("X-ZDR", v)
		}
		resp, err := app.Test(req, 10000)
		if err != nil {
			t.Fatalf("app.Test: %v", err)
		}
		resp.Body.Close()
		docs.mu.Lock()
		defer docs.mu.Unlock()
		if len(docs.uploads) == 0 {
			t.Fatalf("the service was not reached, status %d", resp.StatusCode)
		}
		return docs.uploads[len(docs.uploads)-1].ZDR
	}
	for name, lines := range map[string][]string{
		"false and then true": {"false", "true"},
		"true and then false": {"true", "false"},
		"nothing and a one":   {"", "1"},
		"a one and nothing":   {"1", ""},
		"three, the last":     {"false", "false", "TRUE"},
		"one line":            {"true"},
	} {
		if !zdr(lines...) {
			t.Errorf("%s: the upload is not marked", name)
		}
	}
	for name, lines := range map[string][]string{
		"no line":            nil,
		"false":              {"false"},
		"two that say false": {"false", "0"},
		"an empty line":      {""},
	} {
		if zdr(lines...) {
			t.Errorf("%s: the upload is marked", name)
		}
	}
}

// A tenant named twice is refused on the routes of the documents as it is everywhere.
func TestATenantNamedTwiceIsRefusedOnTheDocumentRoutes(t *testing.T) {
	t.Parallel()

	docs := &fakeDocuments{}
	app, _ := docsServer(t, docs)
	req := httptest.NewRequest(http.MethodPost, docsPath, bytes.NewReader([]byte("x")))
	for k, v := range tenant("X-Org-ID", "") {
		req.Header.Set(k, v)
	}
	req.Header.Add("X-Org-ID", "org-a")
	req.Header.Add("X-Org-ID", "org-b")
	resp, err := app.Test(req, 10000)
	if err != nil {
		t.Fatal(err)
	}
	raw, _ := io.ReadAll(resp.Body)
	resp.Body.Close()
	if resp.StatusCode != 400 || decodeError(t, string(raw)).Error.Code != "invalid_organization" {
		t.Errorf("%d %s, want 400 invalid_organization", resp.StatusCode, raw)
	}
	if docs.uploadCount() != 0 {
		t.Errorf("the service was reached")
	}
}

// A line of the header may be a list, and a folded line is one value with a blank in it: every item counts.
func TestEveryItemOfTheZeroDataRetentionHeaderCounts(t *testing.T) {
	t.Parallel()

	docs := &fakeDocuments{}
	app, _ := docsServer(t, docs)
	zdr := func(value string) bool {
		req := httptest.NewRequest(http.MethodPost, docsPath, bytes.NewReader([]byte("x")))
		for k, v := range tenant() {
			req.Header.Set(k, v)
		}
		req.Header["X-Zdr"] = []string{value}
		resp, err := app.Test(req, 10000)
		if err != nil {
			t.Fatalf("app.Test: %v", err)
		}
		resp.Body.Close()
		docs.mu.Lock()
		defer docs.mu.Unlock()
		return docs.uploads[len(docs.uploads)-1].ZDR
	}
	for _, value := range []string{"true, false", "false, true", "true,1", "false,true", "false true", "TRUE", " 1 ", "no, yes, 1"} {
		if !zdr(value) {
			t.Errorf("%q: the upload is not marked", value)
		}
	}
	for _, value := range []string{"false", "false, 0", "0", "no", "truely", "11", "tru"} {
		if zdr(value) {
			t.Errorf("%q: the upload is marked", value)
		}
	}
}

// rawRequest sends the bytes as they are to the server, over a connection of its own, and returns the status and the
// body of what it answers. A client library cannot be made to send what a hostile one does: a trailer after a body
// that is chunked.
func rawRequest(t *testing.T, app *fiber.App, raw string) (int, string) {
	t.Helper()
	_ = app.Handler() // builds the routes, which app.Test does and ServeConn does not
	client, server := net.Pipe()
	defer client.Close()
	go func() { _ = app.Server().ServeConn(server) }()
	_ = client.SetDeadline(time.Now().Add(20 * time.Second))
	if _, err := client.Write([]byte(raw)); err != nil {
		t.Fatalf("write: %v", err)
	}
	resp, err := http.ReadResponse(bufio.NewReader(client), nil)
	if err != nil {
		t.Fatalf("read the response: %v", err)
	}
	defer resp.Body.Close()
	body, _ := io.ReadAll(resp.Body)
	return resp.StatusCode, string(body)
}

// The server puts the lines of a trailer in the same list as those of the head. A tenant, a user or a posture named
// only in a trailer must not be taken for the gateway's: the tenant is refused, the user is left out, and a trailer
// that asks for zero data retention is believed (the upload is refused for it, and nothing is stored that was meant
// not to be).
func TestATrailerAfterAChunkedBodyDoesNotNameTheTenantTheUserOrThePosture(t *testing.T) {
	t.Parallel()

	head := "POST " + docsPath + " HTTP/1.1\r\nHost: x\r\nX-API-Key: " + serverKey + "\r\nContent-Type: application/zip\r\nTransfer-Encoding: chunked\r\n"
	chunked := func(heads, trailer string) string {
		return head + heads + "\r\n1\r\nx\r\n0\r\n" + trailer + "\r\n"
	}

	docs := &fakeDocuments{}
	app, _ := docsServer(t, docs)
	status, body := rawRequest(t, app, chunked("", "X-Org-ID: org-victim\r\n"))
	if status != 400 || decodeError(t, body).Error.Code != "organization_required" && decodeError(t, body).Error.Code != "invalid_organization" {
		t.Errorf("a tenant only in a trailer: %d %s, want it refused", status, body)
	}
	if docs.uploadCount() != 0 {
		t.Errorf("a tenant named in a trailer reached the service")
	}
	status, body = rawRequest(t, app, chunked("X-Org-ID: org-a\r\n", "X-Org-ID: org-victim\r\n"))
	if status != 400 || decodeError(t, body).Error.Code != "invalid_organization" {
		t.Errorf("a tenant in the head and in a trailer: %d %s, want 400 invalid_organization", status, body)
	}
	if docs.uploadCount() != 0 {
		t.Errorf("a tenant named in the head and in a trailer reached the service")
	}

	// A line that is folded into another is the other's, whatever it reads: a tenant that is only in a continuation
	// line is not named in the head, and a trailer cannot name one either.
	status, body = rawRequest(t, app, chunked("X-Foo: bar\r\n X-Org-ID: org-a\r\n", "X-Org-ID: org-victim\r\n"))
	if status != 400 {
		t.Errorf("a tenant in a fold and in a trailer: %d %s, want it refused", status, body)
	}
	if docs.uploadCount() != 0 {
		t.Errorf("a tenant named in a fold and in a trailer reached the service")
	}

	// The head names the tenant, once, and the trailer the user and the posture.
	status, body = rawRequest(t, app, chunked("X-Org-ID: org-a\r\n", "X-User-ID: user-victim\r\nX-ZDR: true\r\n"))
	if status != 201 {
		t.Fatalf("a trailer with a user and a posture: %d %s", status, body)
	}
	got := docs.uploads[len(docs.uploads)-1]
	if got.OrgID != "org-a" || got.UserID != "" || !got.ZDR {
		t.Errorf("the service was given org %q, user %q, zdr %v; want org-a, no user, and the posture of the trailer", got.OrgID, got.UserID, got.ZDR)
	}
}
