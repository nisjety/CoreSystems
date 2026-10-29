package clarity

import (
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

type transportFunc func(*http.Request) (*http.Response, error)

func (f transportFunc) RoundTrip(r *http.Request) (*http.Response, error) { return f(r) }

const trafficFixture = `[{"metricName":"Traffic","information":[{"totalSessionCount":"12","totalBotSessionCount":"2","distinctUserCount":"7","pagesPerSessionPercentage":1.5,"privateUrl":"https://private.example/"}]},{"metricName":"PopularPages","information":[{"url":"https://private.example/"}]}]`

func bindingFixture(t *testing.T) string {
	t.Helper()
	dir := t.TempDir()
	if err := os.Chmod(dir, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "aquatiq.token"), []byte("synthetic.jwt.signature"), 0600); err != nil {
		t.Fatal(err)
	}
	manifest := `{"version":1,"bindings":[{"organizationId":"org-1","site":"https://aquatiq.com/","projectId":"z1abticqmb","tokenFile":"aquatiq.token","confirmedAt":"2026-10-21T00:00:00Z"}]}`
	path := filepath.Join(dir, "bindings.json")
	if err := os.WriteFile(path, []byte(manifest), 0600); err != nil {
		t.Fatal(err)
	}
	return path
}

func TestClarityProjectBindingCacheAndSecretRedaction(t *testing.T) {
	var calls atomic.Int64
	client := &http.Client{Transport: transportFunc(func(r *http.Request) (*http.Response, error) {
		calls.Add(1)
		if r.URL.String() != "https://www.clarity.ms/export-data/api/v1/project-live-insights?numOfDays=3" || r.Header.Get("Authorization") != "Bearer synthetic.jwt.signature" {
			t.Error("unsafe provider request")
		}
		return &http.Response{StatusCode: 200, Body: io.NopCloser(strings.NewReader(trafficFixture)), Header: http.Header{}}, nil
	})}
	now := time.Date(2026, 10, 21, 12, 0, 0, 0, time.UTC)
	service, err := NewService(bindingFixture(t), client, func() time.Time { return now })
	if err != nil {
		t.Fatal(err)
	}
	got := service.Get(t.Context(), "org-1", "https://aquatiq.com/", 3)
	if got.Status != "connected" || got.Scope != "clarity_project" || got.ProjectID != "z1abticqmb" || got.Metrics == nil || got.Metrics.Sessions == nil || *got.Metrics.Sessions != 12 || *got.Metrics.Users != 7 {
		t.Fatalf("snapshot=%+v", got)
	}
	first := got.UpdatedAt
	now = now.Add(time.Minute)
	got = service.Get(t.Context(), "org-1", "https://aquatiq.com/", 3)
	if got.UpdatedAt != first || calls.Load() != 1 {
		t.Fatal("cached snapshot fetched again or refresh time fabricated")
	}
	*got.Metrics.Sessions = 999
	if copy := service.Get(t.Context(), "org-1", "https://aquatiq.com/", 3); *copy.Metrics.Sessions != 12 {
		t.Fatal("mutable cache projection")
	}
	alias := service.Get(t.Context(), "org-1", "https://www.aquatiq.com/", 3)
	if alias.Site != "https://www.aquatiq.com/" || alias.Status != "connected" || alias.ProjectID != got.ProjectID || calls.Load() != 1 {
		t.Fatal("project alias lost scope or fetched again")
	}
	for _, scope := range []struct{ org, site string }{{"org-other", "https://aquatiq.com/"}, {"org-1", "https://blog.aquatiq.com/"}} {
		if other := service.Get(t.Context(), scope.org, scope.site, 3); other.Reason != "not_configured" || other.ProjectID != "" {
			t.Fatal("project data crossed site or org binding")
		}
	}
	encoded, _ := json.Marshal(got)
	if strings.Contains(string(encoded), "jwt") || strings.Contains(string(encoded), "private.example") {
		t.Fatal("raw secret or URL exposed")
	}
}

func TestClarityExplicitProviderFailuresAndBoundedDailyBudget(t *testing.T) {
	for _, tc := range []struct {
		status       int
		body, reason string
	}{{401, "secret-provider-body", "authorization_required"}, {403, "secret-provider-body", "authorization_required"}, {429, "secret-provider-body", "quota_limited"}, {500, "secret-provider-body", "source_unavailable"}, {200, `{}`, "source_unavailable"}, {200, `[]`, "no_data"}, {200, `[{"metricName":"Traffic","information":[]}]`, "no_data"}, {200, `[{"metricName":"Traffic","information":[{"totalSessionCount":"-1"}]}]`, "source_unavailable"}} {
		client := &http.Client{Transport: transportFunc(func(*http.Request) (*http.Response, error) {
			return &http.Response{StatusCode: tc.status, Body: io.NopCloser(strings.NewReader(tc.body)), Header: http.Header{}}, nil
		})}
		service, err := NewService(bindingFixture(t), client, nil)
		if err != nil {
			t.Fatal(err)
		}
		got := service.Get(t.Context(), "org-1", "https://aquatiq.com/", 3)
		if got.Reason != tc.reason || got.Metrics != nil {
			t.Fatalf("failure=%+v", got)
		}
		encoded, _ := json.Marshal(got)
		if strings.Contains(string(encoded), "secret-provider") {
			t.Fatal("provider failure body leaked")
		}
	}
	now := time.Date(2026, 10, 21, 0, 0, 0, 0, time.UTC)
	calls := 0
	client := &http.Client{Transport: transportFunc(func(*http.Request) (*http.Response, error) {
		calls++
		return nil, errors.New("synthetic secret transport error")
	})}
	service, err := NewService(bindingFixture(t), client, func() time.Time { return now })
	if err != nil {
		t.Fatal(err)
	}
	for index := 0; index < 10; index++ {
		got := service.Get(t.Context(), "org-1", "https://aquatiq.com/", index%3+1)
		if index >= 8 && got.Reason != "quota_limited" {
			t.Fatal("daily request guard missing")
		}
		now = now.Add(2 * time.Hour)
	}
	if calls != 8 {
		t.Fatalf("provider calls=%d, max8perprocess/project/day", calls)
	}
}

func TestClarityRejectsUnsafeManifestAndCredentialFiles(t *testing.T) {
	path := bindingFixture(t)
	for _, change := range []struct{ old, new string }{{`"tokenFile":"aquatiq.token"`, `"tokenFile":"../outside.token"`}, {`"site":"https://aquatiq.com/"`, `"site":"https://127.0.0.1/"`}, {`"confirmedAt":"2026-10-21T00:00:00Z"`, `"confirmedAt":""`}, {`"version":1`, `"version":2`}} {
		data, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		candidate := filepath.Join(t.TempDir(), "bindings.json")
		if err := os.WriteFile(candidate, []byte(strings.Replace(string(data), change.old, change.new, 1)), 0600); err != nil {
			t.Fatal(err)
		}
		if _, err := NewService(candidate, nil, nil); err == nil {
			t.Fatal("unsafe manifest accepted")
		}
	}
	if err := os.Chmod(path, 0644); err != nil {
		t.Fatal(err)
	}
	if _, err := NewService(path, nil, nil); err == nil {
		t.Fatal("unprotected manifest accepted")
	}
	if service, err := NewService("", nil, nil); err != nil || service.Get(t.Context(), "org-1", "https://aquatiq.com/", 3).Reason != "not_configured" {
		t.Fatal("unconfigured service fabricated data")
	}
}
