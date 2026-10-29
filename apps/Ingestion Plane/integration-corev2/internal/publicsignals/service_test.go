package publicsignals

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"strings"
	"sync"
	"testing"
	"time"
)

type roundTrip func(*http.Request) (*http.Response, error)

func (f roundTrip) RoundTrip(r *http.Request) (*http.Response, error) { return f(r) }
func response(code int, body string) *http.Response {
	return &http.Response{StatusCode: code, Body: io.NopCloser(strings.NewReader(body)), Header: http.Header{}}
}

var observed = time.Date(2026, 10, 23, 12, 0, 0, 0, time.UTC)

const indexes = `[{"id":"CC-MAIN-2026-38","cdx-api":"https://evil.example/"},{"id":"CC-MAIN-2026-42","cdx-api":"https://evil.example/"}]`

func cruxFixture(device string) string {
	return `{"record":{"key":{"origin":"https://triodelab.no","formFactor":"` + device + `"},"metrics":{"largest_contentful_paint":{"percentiles":{"p75":1200}},"interaction_to_next_paint":{"percentiles":{"p75":80}},"cumulative_layout_shift":{"percentiles":{"p75":"0.05"}}},"collectionPeriod":{"firstDate":{"year":2026,"month":9,"day":25},"lastDate":{"year":2026,"month":10,"day":22}}},"token":"provider-secret"}`
}

func fixtureService(fn roundTrip) *Service {
	s := New("synthetic-key", &http.Client{Transport: fn}, func() time.Time { return observed })
	s.gate = &requestGate{interval: 0, slot: make(chan struct{}, 1)}
	return s
}
func TestCollectUsesOnlyFixedProvidersAndStripsSecrets(t *testing.T) {
	var mu sync.Mutex
	var paths []string
	s := fixtureService(func(r *http.Request) (*http.Response, error) {
		mu.Lock()
		paths = append(paths, r.URL.String())
		mu.Unlock()
		if r.Header.Get("Authorization") != "" || r.Header.Get("Cookie") != "" {
			t.Error("credential forwarding")
		}
		switch r.URL.Host {
		case "chromeuxreport.googleapis.com":
			if r.Method != "POST" || r.URL.Path != "/v1/records:queryRecord" || r.URL.RawQuery != "" || r.Header.Get("X-Goog-Api-Key") != "synthetic-key" {
				t.Error("unsafe CrUX request")
			}
			var body struct {
				Origin, FormFactor string
				Metrics            []string
			}
			if json.NewDecoder(r.Body).Decode(&body) != nil || body.Origin != "https://triodelab.no" || len(body.Metrics) != 3 {
				t.Error("unbounded CrUX input")
			}
			return response(200, cruxFixture(body.FormFactor)), nil
		case "index.commoncrawl.org":
			if r.Header.Get("X-Goog-Api-Key") != "" || r.Method != "GET" {
				t.Error("secret sent to archive")
			}
			if r.URL.Path == "/collinfo.json" {
				return response(200, indexes), nil
			}
			if r.URL.Path != "/CC-MAIN-2026-42-index" || r.URL.Query().Get("url") != "triodelab.no" || r.URL.Query().Get("matchType") != "host" || r.URL.Query().Get("limit") != "20" || r.URL.Query().Get("fl") != "url,timestamp,status" || len(r.URL.Query()) != 5 {
				t.Error("unsafe archive request")
			}
			return response(200, `{"url":"https://triodelab.no/innsikt?secret=value","timestamp":"20261020123456","status":"200","filename":"private.warc.gz","digest":"private"}`+"\n"+`{"url":"http://triodelab.no/","timestamp":"20261021123456","status":"301"}`), nil
		default:
			t.Fatal("unknown host")
			return nil, nil
		}
	})
	got, err := s.Collect(context.Background(), "https://triodelab.no/")
	if err != nil || len(got.CrUX) != 2 || got.CrUX[0].FormFactor != "PHONE" || got.CrUX[1].FormFactor != "DESKTOP" || got.CrUX[0].Status != "connected" || got.CrUX[0].Metrics == nil || *got.CrUX[0].Metrics.CLS != .05 || got.CommonCrawl.Status != "connected" || len(got.CommonCrawl.Captures) != 2 || got.CommonCrawl.Captures[0].URL != "https://triodelab.no/innsikt" || got.CommonCrawl.Captures[0].CapturedAt != "2026-10-20T12:34:56Z" || got.Source != "public_web_signals" {
		t.Fatalf("got=%+v err=%v", got, err)
	}
	raw, _ := json.Marshal(got)
	for _, v := range []string{"secret", "private", "warc", "synthetic-key"} {
		if strings.Contains(string(raw), v) {
			t.Fatal("raw data leaked")
		}
	}
	if len(paths) != 4 {
		t.Fatalf("requests=%d", len(paths))
	}
}
func TestMissingKeyAndProviderFailuresRemainExplicit(t *testing.T) {
	for _, status := range []int{404, 429, 500} {
		s := fixtureService(func(r *http.Request) (*http.Response, error) {
			if r.URL.Host == "chromeuxreport.googleapis.com" {
				t.Fatal("missing key must not call Google")
			}
			return response(status, "provider-secret"), nil
		})
		s.key = ""
		got, err := s.Collect(context.Background(), "https://triodelab.no/")
		want := "source_unavailable"
		if status == 429 {
			want = "quota_limited"
		}
		if err != nil || got.CrUX[0].Reason != "not_configured" || got.CrUX[0].Metrics != nil || got.CommonCrawl.Reason != want || got.CommonCrawl.Captures == nil {
			t.Fatal(got, err)
		}
	}
}
func TestInputAndProviderEvidenceAreValidated(t *testing.T) {
	for _, site := range []string{"http://triodelab.no/", "https://127.0.0.1/", "https://triodelab.no/path", "https://triodelab.no/?key=x", "https://TRIODELAB.no/", "https://triodelab.no"} {
		s := fixtureService(func(*http.Request) (*http.Response, error) { t.Fatal("invalid site dialed provider"); return nil, nil })
		if _, err := s.Collect(context.Background(), site); err == nil {
			t.Fatalf("accepted %s", site)
		}
	}
	for _, bad := range []string{"foreign", "nan", "negative", "future", "baddate", "allmissing", "oversize"} {
		s := fixtureService(func(r *http.Request) (*http.Response, error) {
			if r.URL.Host == "index.commoncrawl.org" {
				return response(404, ""), nil
			}
			var body struct{ FormFactor string }
			_ = json.NewDecoder(r.Body).Decode(&body)
			value := cruxFixture(body.FormFactor)
			switch bad {
			case "foreign":
				value = strings.ReplaceAll(value, "https://triodelab.no", "https://foreign.no")
			case "nan":
				value = strings.ReplaceAll(value, `"0.05"`, `"NaN"`)
			case "negative":
				value = strings.ReplaceAll(value, `"p75":80`, `"p75":-1`)
			case "future":
				value = strings.ReplaceAll(value, `"month":10,"day":22`, `"month":11,"day":22`)
			case "baddate":
				value = strings.ReplaceAll(value, `"month":9,"day":25`, `"month":9,"day":31`)
			case "allmissing":
				value = `{"record":{"key":{"origin":"https://triodelab.no","formFactor":"` + body.FormFactor + `"},"metrics":{},"collectionPeriod":{"firstDate":{"year":2026,"month":9,"day":25},"lastDate":{"year":2026,"month":10,"day":22}}}}`
			case "oversize":
				value = strings.Repeat("x", 131073)
			}
			return response(200, value), nil
		})
		got, err := s.Collect(context.Background(), "https://triodelab.no/")
		if err != nil || got.CrUX[0].Status != "unavailable" || got.CrUX[0].Metrics != nil {
			t.Fatalf("bad=%s got=%+v err=%v", bad, got, err)
		}
	}
}
