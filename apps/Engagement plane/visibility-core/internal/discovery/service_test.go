package discovery

import (
	"context"
	"errors"
	"sync"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

const testID = "web_11111111111111111111111111111111"
const testSite = "https://triodelab.no/"

type registryStub struct {
	mu       sync.Mutex
	revision int64
}

func (r *registryStub) Resolve(id, site string) (websites.Record, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if id != testID {
		return websites.Record{}, websites.ErrNotFound
	}
	if site != testSite {
		return websites.Record{}, websites.ErrConflict
	}
	return websites.Record{Website: websites.Website{ID: id, Site: site, Revision: r.revision}}, nil
}
func (r *registryStub) change() { r.mu.Lock(); defer r.mu.Unlock(); r.revision++ }

type captureFunc func(context.Context, string) ([]Page, error)

func (f captureFunc) Map(ctx context.Context, site string) ([]Page, error) { return f(ctx, site) }
func settled(t *testing.T, s *Service) Snapshot {
	t.Helper()
	deadline := time.Now().Add(time.Second)
	for time.Now().Before(deadline) {
		v, err := s.Get(testID, testSite)
		if err != nil {
			t.Fatal(err)
		}
		if v.Status != "running" {
			return v
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("discovery did not settle")
	return Snapshot{}
}
func TestDiscoveryDoesNotCaptureOnReadAndIsExplicitPartialEphemeral(t *testing.T) {
	r := &registryStub{revision: 1}
	called := make(chan string, 1)
	s := New(r, captureFunc(func(ctx context.Context, site string) ([]Page, error) {
		called <- site
		return []Page{{URL: site + "innsikt", Title: "Innsikt"}, {URL: site + "innsikt"}, {URL: "https://other.no/"}, {URL: site + "?token=secret"}}, nil
	}), time.Now)
	v, err := s.Get(testID, testSite)
	if err != nil || v.Status != "idle" || !v.Partial || len(v.Pages) != 0 {
		t.Fatalf("idle: %+v %v", v, err)
	}
	select {
	case <-called:
		t.Fatal("GET captured")
	default:
	}
	if _, err := s.Start(testID, testSite, 2); !errors.Is(err, websites.ErrConflict) {
		t.Fatal("stale revision accepted")
	}
	v, err = s.Start(testID, testSite, 1)
	if err != nil || v.Status != "running" {
		t.Fatalf("start %+v %v", v, err)
	}
	v = settled(t, s)
	if v.Status != "complete" || len(v.Pages) != 1 || v.DiscoveredAt == "" || v.Limit != Limit || !v.Partial || v.Source != "quarry_map" {
		t.Fatalf("result %+v", v)
	}
	if <-called != testSite {
		t.Fatal("wrong capture URL")
	}
	v.Pages[0].Title = "mutated"
	again, _ := s.Get(testID, testSite)
	if again.Pages[0].Title != "Innsikt" {
		t.Fatal("mutable snapshot")
	}
	if _, err = s.Start(testID, testSite, 1); !errors.Is(err, ErrCooldown) {
		t.Fatal("no cooldown")
	}
	fresh := New(r, nil, nil)
	v, _ = fresh.Get(testID, testSite)
	if v.Status != "failed" || v.Reason != "not_configured" {
		t.Fatal("unconfigured fabricated completion")
	}
}
func TestDiscoveryDeduplicatesInflightDiscardsStaleAndReportsFailures(t *testing.T) {
	r := &registryStub{revision: 1}
	release := make(chan struct{})
	entered := make(chan struct{})
	s := New(r, captureFunc(func(context.Context, string) ([]Page, error) {
		close(entered)
		<-release
		return []Page{{URL: testSite}}, nil
	}), nil)
	if _, err := s.Start(testID, testSite, 1); err != nil {
		t.Fatal(err)
	}
	<-entered
	if v, err := s.Start(testID, testSite, 1); err != nil || v.Status != "running" {
		t.Fatal("inflight not reused")
	}
	r.change()
	close(release)
	v := settled(t, s)
	if v.Status != "idle" || v.WebsiteRevision != 2 || len(v.Pages) != 0 {
		t.Fatalf("stale result %+v", v)
	}
	for _, reason := range []string{"source_unavailable", "access_denied", "quota_limited", "invalid_source"} {
		s := New(r, captureFunc(func(context.Context, string) ([]Page, error) { return nil, &CaptureError{Reason: reason} }), nil)
		_, _ = s.Start(testID, testSite, 2)
		v := settled(t, s)
		if v.Status != "failed" || v.Reason != reason || len(v.Pages) != 0 || v.DiscoveredAt != "" {
			t.Fatalf("failure %+v", v)
		}
	}
	if _, err := s.Get("web_22222222222222222222222222222222", testSite); !errors.Is(err, websites.ErrNotFound) {
		t.Fatal("foreign website")
	}
	if _, err := s.Get(testID, "https://other.no/"); !errors.Is(err, websites.ErrConflict) {
		t.Fatal("foreign origin")
	}
}
func TestDiscoveryURLAndLabelSafety(t *testing.T) {
	bad := []string{"http://triodelab.no/", "https://news.triodelab.no/", "https://triodelab.no:443/", "https://u:p@triodelab.no/", testSite + "?q=1", testSite + "#token", testSite + "oauth/callback", testSite + "admin", testSite + "a%0ab", testSite + "a%255cb", testSite + "a\\b", testSite + "../admin", testSite + "%61dmin", testSite + "a/../login"}
	for _, u := range bad {
		if SafeURL(testSite, u) {
			t.Fatalf("accepted %q", u)
		}
	}
	if !SafeURL(testSite, testSite+"innsikt/seo") {
		t.Fatal("safe URL rejected")
	}
	pages := []Page{}
	for i := 0; i < Limit+15; i++ {
		pages = append(pages, Page{URL: testSite + time.Unix(int64(i), 0).Format("150405"), Title: "bad\ntitle"})
	}
	got := sanitizePages(testSite, pages)
	if len(got) != Limit || got[0].Title != "" {
		t.Fatal("cap/label")
	}
}
