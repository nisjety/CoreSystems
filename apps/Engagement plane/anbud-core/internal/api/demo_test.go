package api

import (
	"bytes"
	"compress/gzip"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/rs/zerolog"

	"github.com/triodelab/anbud/internal/demo"
	"github.com/triodelab/anbud/internal/eforms/eformstest"
	"github.com/triodelab/anbud/internal/orgcore"
	"github.com/triodelab/anbud/internal/rawstore"
)

var demoNow = time.Date(2026, time.November, 10, 8, 0, 0, 0, time.UTC)

func demoDoc(id string, body []byte) rawstore.Document {
	return rawstore.Document{ID: id, Body: body, SHA256: strings.Repeat("a", 64), FetchedAt: demoNow.Add(-24 * time.Hour)}
}

func demoCorpus() []rawstore.Document {
	return []rawstore.Document{
		demoDoc("2026-100001", eformstest.ContractNotice(eformstest.Notice{Title: "Åpen drift", CPV: "72212000", Deadline: demoNow.Add(20 * 24 * time.Hour)})),
		demoDoc("2026-100002", eformstest.AwardNotice(eformstest.Notice{})),
		demoDoc("2026-100003", []byte("not a notice")),
		demoDoc("2026-100004", eformstest.ContractNotice(eformstest.Notice{SDK: "1.7"})),
	}
}

type stubNotices struct {
	docs []rawstore.Document
	err  error
	wait func(context.Context) error // blocks a scan when set
}

func (s *stubNotices) RecentNotices(ctx context.Context, limit int, fn func(rawstore.Document) error) error {
	if s.wait != nil {
		if err := s.wait(ctx); err != nil {
			return err
		}
	}
	if s.err != nil {
		return s.err
	}
	for i, d := range s.docs {
		if i >= limit {
			break
		}
		if err := fn(d); err != nil {
			return err
		}
	}
	return nil
}

func (s *stubNotices) Notice(_ context.Context, id string) (rawstore.Document, error) {
	if s.err != nil {
		return rawstore.Document{}, s.err
	}
	for _, d := range s.docs {
		if d.ID == id {
			return d, nil
		}
	}
	return rawstore.Document{}, rawstore.ErrNotFound
}

type stubCompanies struct {
	org orgcore.Organisation
	err error
}

func (s *stubCompanies) Lookup(context.Context, string) (orgcore.Organisation, error) {
	return s.org, s.err
}

// logBuffer is a goroutine-safe log sink.
type logBuffer struct {
	mu  sync.Mutex
	buf bytes.Buffer
}

func (l *logBuffer) Write(p []byte) (int, error) {
	l.mu.Lock()
	defer l.mu.Unlock()
	return l.buf.Write(p)
}

func (l *logBuffer) String() string {
	l.mu.Lock()
	defer l.mu.Unlock()
	return l.buf.String()
}

func demoServer(t *testing.T, notices demo.NoticeSource, companies demo.CompanyLookup, opts ...func(*demo.Options)) (*fiber.App, *logBuffer) {
	t.Helper()
	o := demo.Options{Notices: notices, Now: func() time.Time { return demoNow }}
	if companies != nil {
		o.Companies = companies
	}
	for _, f := range opts {
		f(&o)
	}
	svc, err := demo.New(o)
	if err != nil {
		t.Fatalf("demo.New: %v", err)
	}
	logs := &logBuffer{}
	logger := zerolog.New(logs)
	return newServer(ServerConfig{Demo: svc, Logger: &logger}), logs
}

func post(t *testing.T, app *fiber.App, path, body string, headers map[string]string) (int, string, http.Header) {
	t.Helper()
	req := httptest.NewRequest(http.MethodPost, path, strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	if headers == nil {
		headers = map[string]string{"X-API-Key": serverKey}
	}
	for k, v := range headers {
		req.Header.Set(k, v)
	}
	resp, err := app.Test(req, 10000)
	if err != nil {
		// Errorf, not Fatalf: post runs in goroutines too, where Fatalf would only stop the goroutine
		// and leave the test waiting.
		t.Errorf("app.Test: %v", err)
		return 0, "", http.Header{}
	}
	defer resp.Body.Close()
	raw, _ := io.ReadAll(resp.Body)
	return resp.StatusCode, string(raw), resp.Header
}

func decodeMap(t *testing.T, raw string) map[string]any {
	t.Helper()
	var m map[string]any
	if err := json.Unmarshal([]byte(raw), &m); err != nil {
		t.Fatalf("not a JSON object: %q", raw)
	}
	return m
}

func errorOf(t *testing.T, raw string) (code, message string, details any) {
	t.Helper()
	e, _ := decodeMap(t, raw)["error"].(map[string]any)
	if e == nil {
		t.Fatalf("no error envelope in %q", raw)
	}
	code, _ = e["code"].(string)
	message, _ = e["message"].(string)
	return code, message, e["details"]
}

const itProfile = `{"profile":{"cpv":["72000000"]}}`

func TestDemoRoutesNeedTheAPIKey(t *testing.T) {
	t.Parallel()

	app, _ := demoServer(t, &stubNotices{docs: demoCorpus()}, nil)
	for _, path := range []string{"/api/v1/demo/matches", "/api/v1/demo/matches/2026-100001", "/api/v1/demo/company"} {
		for name, headers := range map[string]map[string]string{"no key": {}, "wrong key": {"X-API-Key": "not-the-key"}} {
			if status, raw, _ := post(t, app, path, itProfile, headers); status != 401 {
				t.Errorf("%s, %s: status %d %s, want 401", path, name, status, raw)
			}
		}
	}
}

func TestDemoRoutesAreNotThereWithoutADemoService(t *testing.T) {
	t.Parallel()

	app := newServer(ServerConfig{})
	if status, _, _ := post(t, app, "/api/v1/demo/matches", itProfile, nil); status != 404 {
		t.Errorf("status = %d, want 404: a service with no notice store must not offer the demo", status)
	}
}

func TestMatchesReturnsTheEnvelopeWithFunnelAndResults(t *testing.T) {
	t.Parallel()

	app, _ := demoServer(t, &stubNotices{docs: demoCorpus()}, nil)
	status, raw, header := post(t, app, "/api/v1/demo/matches", itProfile, nil)
	if status != 200 || !strings.HasPrefix(header.Get("Content-Type"), "application/json") {
		t.Fatalf("status %d content type %q: %s", status, header.Get("Content-Type"), raw)
	}
	data, _ := decodeMap(t, raw)["data"].(map[string]any)
	if data == nil {
		t.Fatalf("no data envelope: %s", raw)
	}
	funnel, _ := data["funnel"].(map[string]any)
	if funnel["scanned"] != float64(4) || funnel["awards"] != float64(1) || funnel["unreadable"] != float64(1) || funnel["unsupported"] != float64(1) || funnel["opportunities"] != float64(1) {
		t.Errorf("funnel = %v", funnel)
	}
	results, _ := data["results"].([]any)
	if len(results) != 1 {
		t.Fatalf("results = %v", data["results"])
	}
	first, _ := results[0].(map[string]any)
	if first["notice_id"] != "2026-100001" || first["verdict"] != "pass" || first["band"] != "strong" || first["title"] != "Åpen drift" {
		t.Errorf("first result = %v", first)
	}
	for _, key := range []string{"as_of", "strictness", "gained_by_widening", "funnel", "results"} {
		if _, ok := data[key]; !ok {
			t.Errorf("data has no %q", key)
		}
	}
	for _, marker := range eformstest.PersonalMarkers {
		if strings.Contains(raw, marker) {
			t.Errorf("the response carries %q", marker)
		}
	}
}

func TestMatchesAnswersAnInvalidRequestWithTheFieldsAndNoValues(t *testing.T) {
	t.Parallel()

	const marker = "sk-live-0123456789abcdef"
	app, _ := demoServer(t, &stubNotices{docs: demoCorpus()}, nil)
	status, raw, _ := post(t, app, "/api/v1/demo/matches", `{"profile":{"cpv":["`+marker+`"]},"strictness":"loose","limit":0}`, nil)
	if status != 400 {
		t.Fatalf("status = %d %s", status, raw)
	}
	code, message, details := errorOf(t, raw)
	if code != "validation_failed" || message != "the request is not valid" {
		t.Errorf("code %q message %q", code, message)
	}
	list, _ := details.([]any)
	if len(list) != 3 {
		t.Errorf("details = %v, want 3 field errors", details)
	}
	first, _ := list[0].(map[string]any)
	if first == nil || first["field"] == nil || first["reason"] == nil {
		t.Errorf("a detail has no field and reason: %v", list)
	}
	if strings.Contains(raw, marker) {
		t.Errorf("the error echoes the value: %s", raw)
	}
}

func TestMatchesRefusesABodyOverTheDemoLimit(t *testing.T) {
	t.Parallel()

	app, _ := demoServer(t, &stubNotices{docs: demoCorpus()}, nil)
	big := `{"profile":{"cpv":["72000000"],"keywords":["` + strings.Repeat("x", 70<<10) + `"]}}`
	status, raw, _ := post(t, app, "/api/v1/demo/matches", big, nil)
	if code, _, _ := errorOf(t, raw); status != 413 || code != "payload_too_large" {
		t.Errorf("status %d code %q, want 413 payload_too_large", status, code)
	}
}

func TestMatchesSaysBusyWithARetryAfter(t *testing.T) {
	t.Parallel()

	entered, release := make(chan struct{}, 1), make(chan struct{})
	notices := &stubNotices{docs: demoCorpus(), wait: func(ctx context.Context) error {
		select {
		case entered <- struct{}{}:
			select {
			case <-release:
			case <-ctx.Done():
			}
		default:
		}
		return nil
	}}
	app, _ := demoServer(t, notices, nil, func(o *demo.Options) { o.MaxConcurrent = 1 })

	done := make(chan int, 1)
	go func() {
		status, _, _ := post(t, app, "/api/v1/demo/matches", itProfile, nil)
		done <- status
	}()
	<-entered

	status, raw, header := post(t, app, "/api/v1/demo/matches", itProfile, nil)
	if code, _, _ := errorOf(t, raw); status != 429 || code != "rate_limited" || header.Get("Retry-After") == "" {
		t.Errorf("status %d code %q retry-after %q, want 429 rate_limited with a Retry-After", status, code, header.Get("Retry-After"))
	}
	close(release)
	if got := <-done; got != 200 {
		t.Errorf("the first scan ended with %d", got)
	}
}

func TestMatchesTimesOutWith504(t *testing.T) {
	t.Parallel()

	notices := &stubNotices{wait: func(ctx context.Context) error { <-ctx.Done(); return ctx.Err() }}
	app, _ := demoServer(t, notices, nil, func(o *demo.Options) { o.ScanTimeout = 30 * time.Millisecond })
	status, raw, _ := post(t, app, "/api/v1/demo/matches", itProfile, nil)
	if code, _, _ := errorOf(t, raw); status != 504 || code != "timeout" {
		t.Errorf("status %d code %q, want 504 timeout", status, code)
	}
}

func TestAStoreFailureIsAnInternalErrorThatHidesTheCause(t *testing.T) {
	t.Parallel()

	app, logs := demoServer(t, &stubNotices{err: errors.New("pq: password authentication failed for user anbud_app at 10.0.0.5")}, nil)
	status, raw, _ := post(t, app, "/api/v1/demo/matches", itProfile, nil)
	if code, message, _ := errorOf(t, raw); status != 500 || code != "internal" || message != "internal error" {
		t.Errorf("status %d code %q message %q", status, code, message)
	}
	if strings.Contains(raw, "10.0.0.5") || strings.Contains(raw, "anbud_app") {
		t.Errorf("the response shows the cause: %s", raw)
	}
	if !strings.Contains(logs.String(), "unhandled_error") {
		t.Errorf("the failure was not logged")
	}
}

func TestAuditReturnsTheSheetForOneNotice(t *testing.T) {
	t.Parallel()

	app, _ := demoServer(t, &stubNotices{docs: demoCorpus()}, nil)
	status, raw, _ := post(t, app, "/api/v1/demo/matches/2026-100001", itProfile, nil)
	if status != 200 {
		t.Fatalf("status = %d %s", status, raw)
	}
	data, _ := decodeMap(t, raw)["data"].(map[string]any)
	for _, key := range []string{"result", "notice", "evidence", "normalisation", "parse"} {
		if _, ok := data[key]; !ok {
			t.Errorf("the audit sheet has no %q", key)
		}
	}
	ev, _ := data["evidence"].(map[string]any)
	if ev["source"] != "doffin" || ev["notice_id"] != "2026-100001" || ev["sha256"] != strings.Repeat("a", 64) {
		t.Errorf("evidence = %v", ev)
	}
	for _, marker := range eformstest.PersonalMarkers {
		if strings.Contains(raw, marker) {
			t.Errorf("the audit sheet carries %q", marker)
		}
	}
}

func TestAuditMapsWhatItCannotAssess(t *testing.T) {
	t.Parallel()

	app, _ := demoServer(t, &stubNotices{docs: demoCorpus()}, nil)
	for name, tc := range map[string]struct {
		id     string
		status int
		code   string
	}{
		"not held":        {"2026-999999", 404, "not_found"},
		"a result notice": {"2026-100002", 422, "not_an_opportunity"},
		"not a notice":    {"2026-100003", 422, "unreadable_notice"},
		"no field map":    {"2026-100004", 422, "unsupported_notice"},
		"not an id":       {"not-an-id", 400, "bad_request"},
	} {
		status, raw, _ := post(t, app, "/api/v1/demo/matches/"+tc.id, itProfile, nil)
		if code, _, _ := errorOf(t, raw); status != tc.status || code != tc.code {
			t.Errorf("%s: status %d code %q, want %d %s", name, status, code, tc.status, tc.code)
		}
	}

	_, raw, _ := post(t, app, "/api/v1/demo/matches/2026-100004", itProfile, nil)
	_, _, details := errorOf(t, raw)
	d, _ := details.(map[string]any)
	if d["root"] != "ContractNotice" || d["sdk_version"] != "1.7" || d["flavour"] != "eu" {
		t.Errorf("an unsupported notice names its shape: details = %v", details)
	}
}

func TestAuditValidatesTheProfileToo(t *testing.T) {
	t.Parallel()

	app, _ := demoServer(t, &stubNotices{docs: demoCorpus()}, nil)
	if status, _, _ := post(t, app, "/api/v1/demo/matches/2026-100001", `{"profile":{}}`, nil); status != 400 {
		t.Errorf("status = %d, want 400", status)
	}
}

func TestCompanyReturnsWhatTheRegisterHolds(t *testing.T) {
	t.Parallel()

	companies := &stubCompanies{org: orgcore.Organisation{OrgNumber: "974760673", Name: "REGISTERENHETEN I BRØNNØYSUND", NACE: "84.110"}}
	app, logs := demoServer(t, &stubNotices{}, companies)
	status, raw, _ := post(t, app, "/api/v1/demo/company", `{"org_number":"NO 974 760 673 MVA"}`, nil)
	if status != 200 {
		t.Fatalf("status = %d %s", status, raw)
	}
	data, _ := decodeMap(t, raw)["data"].(map[string]any)
	if data["org_number"] != "974760673" || data["name"] != "REGISTERENHETEN I BRØNNØYSUND" || data["can_bid"] != true || data["nace"] != "84.110" {
		t.Errorf("data = %v", data)
	}
	// The number is in the body, not the path, so it is not in the access log.
	if strings.Contains(logs.String(), "974760673") {
		t.Errorf("the log holds the organisation number: %s", logs.String())
	}
}

func TestCompanyMapsEveryFailure(t *testing.T) {
	t.Parallel()

	for name, tc := range map[string]struct {
		body      string
		companies demo.CompanyLookup
		status    int
		code      string
	}{
		"not configured":      {`{"org_number":"974760673"}`, nil, 503, "not_configured"},
		"not in the register": {`{"org_number":"974760673"}`, &stubCompanies{err: orgcore.ErrNotFound}, 404, "not_found"},
		"org-core is down":    {`{"org_number":"974760673"}`, &stubCompanies{err: errors.New("dial tcp 10.0.0.9:8080: connection refused")}, 502, "upstream_unavailable"},
		"credential refused":  {`{"org_number":"974760673"}`, &stubCompanies{err: orgcore.ErrUnauthorized}, 502, "upstream_unavailable"},
		"bad number":          {`{"org_number":"974760674"}`, &stubCompanies{}, 400, "validation_failed"},
		"no number":           {`{}`, &stubCompanies{}, 400, "validation_failed"},
		"not json":            {`{`, &stubCompanies{}, 400, "validation_failed"},
		"unknown field":       {`{"org_number":"974760673","x":1}`, &stubCompanies{}, 400, "validation_failed"},
	} {
		app, _ := demoServer(t, &stubNotices{}, tc.companies)
		status, raw, _ := post(t, app, "/api/v1/demo/company", tc.body, nil)
		if code, _, _ := errorOf(t, raw); status != tc.status || code != tc.code {
			t.Errorf("%s: status %d code %q, want %d %s", name, status, code, tc.status, tc.code)
		}
		if strings.Contains(raw, "10.0.0.9") {
			t.Errorf("%s: the response shows the cause: %s", name, raw)
		}
	}
}

func TestACompanyFailureIsLoggedWithItsCauseButNotItsNumber(t *testing.T) {
	t.Parallel()

	app, logs := demoServer(t, &stubNotices{}, &stubCompanies{err: errors.New("dial tcp 10.0.0.9:8080: connection refused")})
	post(t, app, "/api/v1/demo/company", `{"org_number":"974760673"}`, nil)
	if !strings.Contains(logs.String(), "10.0.0.9") {
		t.Errorf("the cause is not in the log: %s", logs.String())
	}
	if strings.Contains(logs.String(), "974760673") {
		t.Errorf("the log holds the organisation number")
	}
}

// The body limit is 64 KiB. The last byte it accepts is a valid request; one more is refused.
func TestTheDemoBodyLimitIsExact(t *testing.T) {
	t.Parallel()

	app, _ := demoServer(t, &stubNotices{docs: demoCorpus()}, nil)
	pad := func(n int) string {
		base := `{"profile":{"cpv":["72000000"]}}`
		return base + strings.Repeat(" ", n-len(base))
	}
	if status, raw, _ := post(t, app, "/api/v1/demo/matches", pad(maxDemoBody), nil); status != 200 {
		t.Errorf("a body of exactly %d bytes: status %d %s, want 200", maxDemoBody, status, raw)
	}
	if status, _, _ := post(t, app, "/api/v1/demo/matches", pad(maxDemoBody+1), nil); status != 413 {
		t.Errorf("a body of %d bytes: status %d, want 413", maxDemoBody+1, status)
	}
	for _, path := range []string{"/api/v1/demo/matches/2026-100001", "/api/v1/demo/company"} {
		if status, _, _ := post(t, app, path, strings.Repeat(" ", maxDemoBody+1), nil); status != 413 {
			t.Errorf("%s: a body over the limit gave %d, want 413", path, status)
		}
	}
}

// The demo handlers read the body as it came and inflate nothing, as the upload does: whatever lets a compressed
// body by, a compressed profile is not a profile. Mounted bare here, with nothing in front of them that would refuse
// the encoding; if they inflated, each of these would be a valid request.
func TestTheDemoHandlersReadTheBodyRawAndInflateNothing(t *testing.T) {
	t.Parallel()

	svc, err := demo.New(demo.Options{
		Notices: &stubNotices{docs: demoCorpus()}, Companies: &stubCompanies{org: orgcore.Organisation{OrgNumber: "974760673", Name: "X"}},
		Now: func() time.Time { return demoNow },
	})
	if err != nil {
		t.Fatal(err)
	}
	app := fiber.New()
	app.Post("/matches", matchesHandler(svc, nil))
	app.Post("/matches/:notice_id", auditHandler(svc, nil))
	app.Post("/company", companyHandler(svc, nil))

	gzipped := func(s string) []byte {
		var b bytes.Buffer
		zw := gzip.NewWriter(&b)
		_, _ = zw.Write([]byte(s))
		_ = zw.Close()
		return b.Bytes()
	}
	for path, body := range map[string]string{
		"/matches":             `{"profile":{"cpv":["72212000"]}}`,
		"/matches/2026-100001": `{"profile":{"cpv":["72212000"]}}`,
		"/company":             `{"org_number":"974760673"}`,
	} {
		// The same body, sent as it is, is a valid request.
		plain := httptest.NewRequest(http.MethodPost, path, strings.NewReader(body))
		plain.Header.Set("Content-Type", "application/json")
		resp, err := app.Test(plain, 10000)
		if err != nil {
			t.Fatal(err)
		}
		resp.Body.Close()
		if resp.StatusCode != 200 {
			t.Fatalf("%s: the body as it is: status %d, the test needs it to be a valid request", path, resp.StatusCode)
		}
		packed := httptest.NewRequest(http.MethodPost, path, bytes.NewReader(gzipped(body)))
		packed.Header.Set("Content-Type", "application/json")
		packed.Header.Set("Content-Encoding", "gzip")
		resp, err = app.Test(packed, 10000)
		if err != nil {
			t.Fatal(err)
		}
		raw, _ := io.ReadAll(resp.Body)
		resp.Body.Close()
		if resp.StatusCode != 400 {
			t.Errorf("%s: the body compressed: status %d %s, want 400: it was inflated", path, resp.StatusCode, raw)
		}
		// The size is judged by the body as it came: a small body that would be large if it were inflated is not
		// too large, it is only not a request.
		big := httptest.NewRequest(http.MethodPost, path, bytes.NewReader(gzipped(body+strings.Repeat(" ", 4*maxDemoBody))))
		big.Header.Set("Content-Type", "application/json")
		big.Header.Set("Content-Encoding", "gzip")
		resp, err = app.Test(big, 10000)
		if err != nil {
			t.Fatal(err)
		}
		raw, _ = io.ReadAll(resp.Body)
		resp.Body.Close()
		if resp.StatusCode != 400 {
			t.Errorf("%s: a body that is large only when inflated: status %d %s, want 400, not 413", path, resp.StatusCode, raw)
		}
	}
}

// A tenant named only in a trailer is not the tenant of a demo request either: the documents of another
// organisation are not read for it.
func TestATenantNamedInATrailerIsRefusedOnTheDemoRoutes(t *testing.T) {
	t.Parallel()

	app, _ := demoServer(t, &stubNotices{docs: demoCorpus()}, nil)
	body := `{"profile":{"cpv":["72212000"]}}`
	raw := "POST /api/v1/demo/matches HTTP/1.1\r\nHost: x\r\nX-API-Key: " + serverKey + "\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n" +
		fmt.Sprintf("%x\r\n%s\r\n0\r\nX-Org-ID: org-victim\r\n\r\n", len(body), body)
	status, got := rawRequest(t, app, raw)
	if status != 400 || decodeError(t, got).Error.Code != "invalid_organization" {
		t.Errorf("%d %s, want 400 invalid_organization", status, got)
	}
}

func (s *stubNotices) CataloguePage(ctx context.Context, before int64, limit int) ([]rawstore.CatalogueDocument, error) {
	if s.wait != nil {
		if err := s.wait(ctx); err != nil {
			return nil, err
		}
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	out := []rawstore.CatalogueDocument{}
	for i, d := range s.docs {
		seq := int64(len(s.docs) - i)
		if before != 0 && seq >= before {
			continue
		}
		out = append(out, rawstore.CatalogueDocument{Sequence: seq, Document: d})
		if len(out) == limit {
			break
		}
	}
	return out, s.err
}
func TestCatalogueHTTPRequiresKeyButNotProfile(t *testing.T) {
	app, _ := demoServer(t, &stubNotices{docs: demoCorpus()}, nil)
	status, body, _ := post(t, app, "/api/v1/notices/search", `{}`, nil)
	if status != 200 || !strings.Contains(body, `"results"`) {
		t.Fatalf("catalogue status=%d body=%s", status, body)
	}
	status, _, _ = post(t, app, "/api/v1/notices/search", `{}`, map[string]string{})
	if status != 401 {
		t.Fatalf("unauthenticated catalogue status=%d", status)
	}
	status, _, _ = post(t, app, "/api/v1/notices/search", `{"cursor":"-5"}`, nil)
	if status != 400 {
		t.Fatalf("bad cursor status=%d", status)
	}
}

func TestCatalogueDetailHTTPNeedsNoProfileAndKeepsAuthentication(t *testing.T) {
	app, _ := demoServer(t, &stubNotices{docs: demoCorpus()}, nil)
	status, body, _ := post(t, app, "/api/v1/notices/2026-100001", `{}`, nil)
	if status != 200 || !strings.Contains(body, `"description"`) || !strings.Contains(body, `"evidence"`) {
		t.Fatalf("status=%d body=%s", status, body)
	}
	status, _, _ = post(t, app, "/api/v1/notices/2026-100001", `{}`, map[string]string{})
	if status != 401 {
		t.Fatalf("unauthenticated detail=%d", status)
	}
	status, _, _ = post(t, app, "/api/v1/notices/2026-999999", `{}`, nil)
	if status != 404 {
		t.Fatalf("missing detail=%d", status)
	}
}
