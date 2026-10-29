package main

import (
	"archive/zip"
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/rs/zerolog"

	"github.com/triodelab/anbud/internal/api"
	"github.com/triodelab/anbud/internal/config"
	"github.com/triodelab/anbud/internal/db/dbtest"
	"github.com/triodelab/anbud/internal/docstore"
	"github.com/triodelab/anbud/internal/doffinsync"
	"github.com/triodelab/anbud/internal/eforms/eformstest"
	"github.com/triodelab/anbud/internal/rawstore"
	"github.com/triodelab/anbud/internal/secret"
)

const noticeWithDocuments = "2026-100001"

// archiveOfTenderDocuments is what a company downloads from a tender platform: the specification as a
// PDF (read by the child process, which is this test binary), a contract as a Word file, and a text.
func archiveOfTenderDocuments(t *testing.T) []byte {
	t.Helper()
	pdf, err := os.ReadFile("../../internal/docread/testdata/konkurransegrunnlag.pdf")
	if err != nil {
		t.Fatalf("read the test PDF: %v", err)
	}
	var buf bytes.Buffer
	zw := zip.NewWriter(&buf)
	add := func(name string, data []byte) {
		w, err := zw.Create(name)
		if err != nil {
			t.Fatal(err)
		}
		if _, err := w.Write(data); err != nil {
			t.Fatal(err)
		}
	}
	add("konkurransegrunnlag.pdf", pdf)
	add("vedlegg/notat.txt", []byte("Dette er et internt notat uten krav.\n\nHEMMELIG-NOTAT-4711 er bare for oss."))
	add("vedlegg/bilde.png", []byte("\x89PNG\r\n\x1a\n.....not a real picture"))
	if err := zw.Close(); err != nil {
		t.Fatal(err)
	}
	return buf.Bytes()
}

// tenderDocsServer is the service as production wires it, over a fresh database that holds two notices.
func tenderDocsServer(t *testing.T) (*fiber.App, *pgxpool.Pool) {
	t.Helper()
	pool := dbtest.Open(t)
	store := rawstore.New(pool)
	ctx := context.Background()
	day := time.Date(2026, time.October, 1, 0, 0, 0, 0, time.UTC)
	run, err := store.BeginRun(ctx, doffinsync.RunStart{Mode: doffinsync.ModeSync, From: day, To: day})
	if err != nil {
		t.Fatalf("BeginRun: %v", err)
	}
	open := time.Now().UTC().Add(20 * 24 * time.Hour)
	storeNotice(t, store, run, noticeWithDocuments, eformstest.ContractNotice(eformstest.Notice{Title: "Åpen drift", CPV: "72212000", Deadline: open, PointToDocuments: true}))
	storeNotice(t, store, run, "2026-100002", eformstest.ContractNotice(eformstest.Notice{Title: "Uten opplasting", CPV: "72212000", Deadline: open}))

	cfg := config.Config{TenderDocs: config.TenderDocs{Enabled: true, Retention: 48 * time.Hour, MaxArchiveBytes: 8 << 20}}
	logger := zerolog.Nop()
	tender, err := buildTenderDocs(cfg, docstore.New(pool), store, logger)
	if err != nil || tender == nil {
		t.Fatalf("buildTenderDocs: %v", err)
	}
	demoSvc, err := buildDemo(cfg, store, tender, logger)
	if err != nil {
		t.Fatalf("buildDemo: %v", err)
	}
	if err := demoSvc.PrepareCatalogue(context.Background()); err != nil {
		t.Fatal(err)
	}
	return api.NewServer(api.ServerConfig{APIKey: secret.Value(e2eKey), Demo: demoSvc, Documents: tender}), pool
}

// rowsOf returns the single integer a query yields.
func rowsOf(t *testing.T, pool *pgxpool.Pool, query string) int {
	t.Helper()
	var n int
	if err := pool.QueryRow(context.Background(), query).Scan(&n); err != nil {
		t.Fatalf("%s: %v", query, err)
	}
	return n
}

func request(t *testing.T, app *fiber.App, method, path string, body []byte, headers map[string]string) (int, []byte) {
	t.Helper()
	req := httptest.NewRequest(method, path, bytes.NewReader(body))
	req.Header.Set("X-API-Key", e2eKey)
	for k, v := range headers {
		req.Header.Set(k, v)
	}
	resp, err := app.Test(req, 60000)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	defer resp.Body.Close()
	raw, _ := io.ReadAll(resp.Body)
	return resp.StatusCode, raw
}

func asMap(t *testing.T, raw []byte) map[string]any {
	t.Helper()
	var m map[string]any
	if err := json.Unmarshal(raw, &m); err != nil {
		t.Fatalf("not JSON: %q", raw)
	}
	return m
}

func at(m map[string]any, path ...string) any {
	var cur any = m
	for _, p := range path {
		next, _ := cur.(map[string]any)
		if next == nil {
			return nil
		}
		cur = next[p]
	}
	return cur
}

func TestTenderDocumentsEndToEndAgainstARealDatabase(t *testing.T) {
	app, pool := tenderDocsServer(t)
	rows := func(query string) int { return rowsOf(t, pool, query) }
	archive := archiveOfTenderDocuments(t)
	docsPath := "/api/v1/notices/" + noticeWithDocuments + "/documents"
	orgA := map[string]string{"X-Org-ID": "org-a", "X-User-ID": "user-1", "Content-Type": "application/zip", "X-Archive-Name": "Konkurransegrunnlag%20%C3%B8st.zip"}

	// Upload. The PDF is read by a child process, the text in process, the picture is listed and not read.
	marker := childMarker(os.TempDir(), os.Getpid())
	_ = os.Remove(marker)
	t.Cleanup(func() { _ = os.Remove(marker) })
	status, raw := request(t, app, http.MethodPost, docsPath, archive, orgA)
	if status != 201 {
		t.Fatalf("upload: %d %s", status, raw)
	}
	if _, err := os.Stat(marker); err != nil {
		t.Errorf("no child process read the PDF: %v. The service reads PDFs in a child so that a bug in the reader does not hold its environment", err)
	}
	// The retention is the configured one: 48 hours.
	if got := rows(`SELECT extract(epoch FROM expires_at - created_at)::bigint FROM tenant.tender_uploads`); got != 48*3600 {
		t.Errorf("an upload expires after %d seconds, want %d", got, 48*3600)
	}
	up := asMap(t, raw)
	if at(up, "data", "read") != float64(2) || at(up, "data", "unread") != float64(1) || at(up, "data", "archive_name") != "Konkurransegrunnlag øst.zip" {
		t.Errorf("upload = %s", raw)
	}
	findings, _ := at(up, "data", "findings").([]any)
	byCapability := map[string]string{}
	for _, f := range findings {
		fm := f.(map[string]any)
		byCapability[fm["capability"].(string)] = fm["class"].(string)
	}
	for capability, class := range map[string]string{"iso9001": "requirement", "startbank": "requirement", "tax_certificate": "requirement", "miljofyrtarn": "negated"} {
		if byCapability[capability] != class {
			t.Errorf("finding %s = %q, want %q (all: %v)", capability, byCapability[capability], class, byCapability)
		}
	}
	id := int(at(up, "data", "id").(float64))

	// The same archive again is that upload.
	if status, raw := request(t, app, http.MethodPost, docsPath, archive, orgA); status != 200 || int(at(asMap(t, raw), "data", "id").(float64)) != id {
		t.Errorf("the same archive again: %d %s", status, raw)
	}
	if n := rows(`SELECT count(*) FROM tenant.tender_uploads`); n != 1 {
		t.Errorf("rows = %d, want 1", n)
	}

	// The matching for that organisation is judged by the documents; for another organisation and for
	// no organisation it is not.
	profile := []byte(`{"profile":{"cpv":["72000000"]}}`)
	hasDocumentCheck := func(raw []byte) bool {
		results, _ := at(asMap(t, raw), "data", "results").([]any)
		for _, r := range results {
			rm := r.(map[string]any)
			if rm["notice_id"] != noticeWithDocuments {
				continue
			}
			gate, _ := rm["gate"].([]any)
			for _, c := range gate {
				if cm := c.(map[string]any); cm["rule"] == "document_requirement" {
					return true
				}
			}
		}
		return false
	}
	_, forA := request(t, app, http.MethodPost, "/api/v1/demo/matches", profile, map[string]string{"X-Org-ID": "org-a", "Content-Type": "application/json"})
	_, forB := request(t, app, http.MethodPost, "/api/v1/demo/matches", profile, map[string]string{"X-Org-ID": "org-b", "Content-Type": "application/json"})
	_, forNobody := request(t, app, http.MethodPost, "/api/v1/demo/matches", profile, map[string]string{"Content-Type": "application/json"})
	if !hasDocumentCheck(forA) {
		t.Errorf("the organisation that uploaded is not judged by its documents: %s", forA)
	}
	if hasDocumentCheck(forB) || hasDocumentCheck(forNobody) {
		t.Errorf("documents of org-a were used for someone else: org-b %v nobody %v", hasDocumentCheck(forB), hasDocumentCheck(forNobody))
	}
	if at(asMap(t, forA), "data", "documents", "notices") != float64(1) {
		t.Errorf("documents status for org-a = %v", at(asMap(t, forA), "data", "documents"))
	}

	// The audit shows the passages, with where they are.
	status, raw = request(t, app, http.MethodPost, "/api/v1/demo/matches/"+noticeWithDocuments, profile, map[string]string{"X-Org-ID": "org-a", "Content-Type": "application/json"})
	if status != 200 {
		t.Fatalf("audit: %d %s", status, raw)
	}
	auditFindings, _ := at(asMap(t, raw), "data", "documents", "findings").([]any)
	if len(auditFindings) < 4 || !strings.Contains(string(raw), `"document":"konkurransegrunnlag.pdf"`) {
		t.Errorf("audit documents = %s", raw)
	}

	// Another organisation sees nothing of it, and cannot delete it.
	orgB := map[string]string{"X-Org-ID": "org-b"}
	if _, raw := request(t, app, http.MethodGet, docsPath, nil, orgB); !strings.Contains(string(raw), `"uploads":[]`) {
		t.Errorf("org-b lists %s", raw)
	}
	if status, _ := request(t, app, http.MethodDelete, docsPath+"/"+itoa(id), nil, orgB); status != 404 {
		t.Errorf("org-b deleted it: %d", status)
	}
	if n := rows(`SELECT count(*) FROM tenant.tender_uploads`); n != 1 {
		t.Errorf("rows = %d after org-b's delete, want 1", n)
	}

	// A notice that is not held, an organisation in zero-data-retention mode, and a body that is no
	// archive: refused, and nothing is stored for any of them.
	if status, _ := request(t, app, http.MethodPost, "/api/v1/notices/2026-999999/documents", archive, orgA); status != 404 {
		t.Errorf("a notice that is not held: %d", status)
	}
	zdr := map[string]string{"X-Org-ID": "org-z", "Content-Type": "application/zip", "X-ZDR": "true"}
	if status, raw := request(t, app, http.MethodPost, docsPath, archive, zdr); status != 409 || !strings.Contains(string(raw), "zdr_persistence_forbidden") {
		t.Errorf("a ZDR upload: %d %s", status, raw)
	}
	if status, raw := request(t, app, http.MethodPost, docsPath, []byte("this is not a zip"), orgA); status != 422 || !strings.Contains(string(raw), "not_a_zip") {
		t.Errorf("not a zip: %d %s", status, raw)
	}
	if n := rows(`SELECT count(*) FROM tenant.tender_uploads`); n != 1 {
		t.Errorf("rows = %d: a refused upload left something behind", n)
	}

	// Delete: the owner's, and then the documents no longer count.
	if status, _ := request(t, app, http.MethodDelete, docsPath+"/"+itoa(id), nil, orgA); status != 204 {
		t.Errorf("delete: %d", status)
	}
	_, forAAfter := request(t, app, http.MethodPost, "/api/v1/demo/matches", profile, map[string]string{"X-Org-ID": "org-a", "Content-Type": "application/json"})
	if hasDocumentCheck(forAAfter) {
		t.Errorf("a deleted upload is still used")
	}
	if n := rows(`SELECT count(*) FROM tenant.tender_uploads`); n != 0 {
		t.Errorf("rows = %d after delete, want 0", n)
	}
}

func itoa(n int) string {
	b, _ := json.Marshal(n)
	return string(b)
}

// Nothing of a document is kept but the manifest and the sentences that name a capability: the rest of
// the text is in no table, and not in the answer either.
func TestTenderDocumentsKeepNoTextThatNamesNothing(t *testing.T) {
	app, pool := tenderDocsServer(t)
	docsPath := "/api/v1/notices/" + noticeWithDocuments + "/documents"
	status, raw := request(t, app, http.MethodPost, docsPath, archiveOfTenderDocuments(t), map[string]string{"X-Org-ID": "org-a", "Content-Type": "application/zip"})
	if status != 201 {
		t.Fatalf("upload: %d %s", status, raw)
	}
	var stored string
	if err := pool.QueryRow(context.Background(), `SELECT documents::text || findings::text || archive_name FROM tenant.tender_uploads`).Scan(&stored); err != nil {
		t.Fatal(err)
	}
	for _, notNamed := range []string{"HEMMELIG-NOTAT-4711", "Blåbærsyltetøy", "Pris 40 prosent", "internt notat"} {
		if strings.Contains(stored, notNamed) || strings.Contains(string(raw), notNamed) {
			t.Errorf("%q is in a table or in the answer", notNamed)
		}
	}
	for _, named := range []string{"ISO 9001", "StartBANK", "Skatteattest", "Miljøfyrtårn"} {
		if !strings.Contains(stored, named) {
			t.Errorf("the sentence that names %q is not kept", named)
		}
	}
}

// An upload that has expired is gone to every read before it is purged, and the purge removes it.
func TestAnExpiredUploadIsGoneAndIsPurged(t *testing.T) {
	pool := dbtest.Open(t)
	store := rawstore.New(pool)
	ctx := context.Background()
	day := time.Date(2026, time.October, 1, 0, 0, 0, 0, time.UTC)
	run, err := store.BeginRun(ctx, doffinsync.RunStart{Mode: doffinsync.ModeSync, From: day, To: day})
	if err != nil {
		t.Fatal(err)
	}
	storeNotice(t, store, run, noticeWithDocuments, eformstest.ContractNotice(eformstest.Notice{Title: "Åpen drift", CPV: "72212000"}))
	cfg := config.Config{TenderDocs: config.TenderDocs{Enabled: true, Retention: 48 * time.Hour, MaxArchiveBytes: 8 << 20}}
	tender, err := buildTenderDocs(cfg, docstore.New(pool), store, zerolog.Nop())
	if err != nil {
		t.Fatal(err)
	}
	app := api.NewServer(api.ServerConfig{APIKey: secret.Value(e2eKey), Documents: tender})
	docsPath := "/api/v1/notices/" + noticeWithDocuments + "/documents"
	orgA := map[string]string{"X-Org-ID": "org-a", "Content-Type": "application/zip"}
	if status, raw := request(t, app, http.MethodPost, docsPath, archiveOfTenderDocuments(t), orgA); status != 201 {
		t.Fatalf("upload: %d %s", status, raw)
	}
	if _, err := pool.Exec(ctx, `UPDATE tenant.tender_uploads SET created_at = now() - interval '3 days', expires_at = now() - interval '1 day'`); err != nil {
		t.Fatalf("age the upload: %v", err)
	}
	if _, raw := request(t, app, http.MethodGet, docsPath, nil, map[string]string{"X-Org-ID": "org-a"}); !strings.Contains(string(raw), `"uploads":[]`) {
		t.Errorf("an expired upload is listed: %s", raw)
	}
	removed, err := tender.PurgeExpired(ctx)
	if err != nil || removed != 1 {
		t.Errorf("PurgeExpired = %d, %v; want 1", removed, err)
	}
	// And the same archive can be uploaded again.
	if status, raw := request(t, app, http.MethodPost, docsPath, archiveOfTenderDocuments(t), orgA); status != 201 {
		t.Errorf("upload after the purge: %d %s", status, raw)
	}
}

// Many uploads at once, through the HTTP layer, the service, the store and a real database: of forty archives for a
// notice that may hold ten, ten are stored and thirty are refused with 409 `limit_reached`, however they interleave. The
// count the service makes before it reads an archive is passed by every upload that is under way at once, and was
// all there was until 2026-10-13; the store's, made under a lock of the organisation in the transaction that stores
// the upload, is not. The slots for reading are two, and a busy one (429) is tried again.
func TestUploadsThatArriveAtOnceDoNotPassTheLimitAgainstARealDatabase(t *testing.T) {
	app, pool := tenderDocsServer(t)
	docsPath := "/api/v1/notices/" + noticeWithDocuments + "/documents"
	headers := map[string]string{"X-Org-ID": "org-burst", "Content-Type": "application/zip"}
	if status, raw := request(t, app, http.MethodGet, "/health", nil, nil); status != 200 { // the routes are built by the first request
		t.Fatalf("health: %d %s", status, raw)
	}

	const sent, limit = 40, 10 // the service's own limit for a notice
	var wg sync.WaitGroup
	var mu sync.Mutex
	statuses := map[int]int{}
	for i := 0; i < sent; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			var archive bytes.Buffer
			zw := zip.NewWriter(&archive)
			w, _ := zw.Create("a.txt")
			_, _ = fmt.Fprintf(w, "archive number %d", i)
			_ = zw.Close()

			status := 0
			for try := 0; try < 2000 && (try == 0 || status == http.StatusTooManyRequests); try++ {
				if try > 0 {
					time.Sleep(3 * time.Millisecond)
				}
				req := httptest.NewRequest(http.MethodPost, docsPath, bytes.NewReader(archive.Bytes()))
				req.Header.Set("X-API-Key", e2eKey)
				for k, v := range headers {
					req.Header.Set(k, v)
				}
				resp, err := app.Test(req, 60000)
				if err != nil {
					t.Errorf("upload %d: %v", i, err)
					return
				}
				status = resp.StatusCode
				_ = resp.Body.Close()
			}
			mu.Lock()
			statuses[status]++
			mu.Unlock()
		}()
		time.Sleep(2 * time.Millisecond) // a stream, and not one burst
	}
	wg.Wait()

	if statuses[http.StatusCreated] != limit || statuses[http.StatusConflict] != sent-limit || len(statuses) != 2 {
		t.Errorf("answers %v; want %d stored (201) and %d refused (409) and nothing else", statuses, limit, sent-limit)
	}
	if n := rowsOf(t, pool, `SELECT count(*) FROM tenant.tender_uploads WHERE org_id = 'org-burst'`); n != limit {
		t.Errorf("the notice holds %d uploads, want %d", n, limit)
	}
}
