package sitesources

import (
	"context"
	"errors"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"testing"
	"time"
)

type fakeAuthority struct {
	authorization Authorization
	report        Report
	err, bindErr  error
	calls         int
	after         func()
}

func (a *fakeAuthority) Authorize(ctx context.Context, _ websites.Record) (Authorization, error) {
	if a.after != nil {
		a.after()
	}
	if err := ctx.Err(); err != nil {
		return Authorization{}, err
	}
	return a.authorization, a.err
}
func (a *fakeAuthority) CollectOAuth(context.Context, websites.Record, Authorization) (Report, error) {
	a.calls++
	return a.report, a.err
}
func (a *fakeAuthority) BindSnapshot(_ websites.Record, _ Authorization, report Report) error {
	if a.bindErr != nil {
		return a.bindErr
	}
	a.authorization.SnapshotObservedAt = report.ObservedAt
	a.authorization.SnapshotDigest = Fingerprint(report)
	return nil
}
func mapped(t *testing.T) (*Service, *fakeRegistry, *fakeCollector, *fakeAuthority) {
	s, r, c, _ := fixture(t)
	report := clone(c.report)
	report.Algolia.Region = "provider"
	a := &fakeAuthority{authorization: Authorization{Managed: true, Active: true, Fingerprint: "mapping-1", Generation: "grant-1"}, report: report}
	s.oauth = a
	return s, r, c, a
}
func TestOAuthPreferredAndCachedReadsRequireLiveGrantAndExactReceipt(t *testing.T) {
	s, r, c, a := mapped(t)
	w := r.record.Website
	if _, err := s.Sync(t.Context(), w.ID, w.Site, w.Revision, "algolia"); err != nil {
		t.Fatal(err)
	}
	got, err := s.Get(w.ID, w.Site, "algolia")
	if err != nil || got.Status != "connected" || c.calls != 0 || a.calls != 1 {
		t.Fatal(got, err, c.calls, a.calls)
	}
	a.authorization.Active = false
	got, err = s.Get(w.ID, w.Site, "algolia")
	if err != nil || got.Status != "not_loaded" || got.Algolia != nil {
		t.Fatal("revocation leaked cache", got, err)
	}
	if _, err := s.Sync(t.Context(), w.ID, w.Site, w.Revision, "algolia"); !errors.Is(err, ErrCooldown) {
		t.Fatal(err)
	}
	s.now = func() time.Time { return time.Date(2026, 10, 23, 12, 2, 0, 0, time.UTC) }
	if _, err := s.Sync(t.Context(), w.ID, w.Site, w.Revision, "algolia"); !errors.Is(err, ErrUnavailable) || c.calls != 0 {
		t.Fatal("tombstone fell back", err, c.calls)
	}
	a.authorization.Active = true
	a.authorization.Generation = "grant-2"
	a.authorization.SnapshotObservedAt = ""
	a.authorization.SnapshotDigest = ""
	got, err = s.Get(w.ID, w.Site, "algolia")
	if err != nil || got.Status != "not_loaded" {
		t.Fatal(got, err)
	}
	a.err = errors.New("access metadata outage")
	got, err = s.Get(w.ID, w.Site, "algolia")
	if err != nil || got.Status != "not_loaded" {
		t.Fatal(got, err)
	}
}
func TestFailedPrivateBindCannotAuthorizeDifferentDataWithReusedTimestamp(t *testing.T) {
	s, r, _, a := mapped(t)
	w := r.record.Website
	if _, err := s.Sync(t.Context(), w.ID, w.Site, w.Revision, "algolia"); err != nil {
		t.Fatal(err)
	}
	s.now = func() time.Time { return time.Date(2026, 10, 23, 12, 2, 0, 0, time.UTC) }
	a.report.Algolia.Analytics.Searches = integer(20)
	a.report.Algolia.Analytics.NoResultSearches = integer(4)
	a.bindErr = errors.New("mapping receipt disk write failed")
	if _, err := s.Sync(t.Context(), w.ID, w.Site, w.Revision, "algolia"); !errors.Is(err, ErrUnavailable) {
		t.Fatal(err)
	}
	got, err := s.Get(w.ID, w.Site, "algolia")
	if err != nil || got.Status != "not_loaded" || got.Algolia != nil {
		t.Fatal("old proof authorized new data", got, err)
	}
}
func TestAuthorizationNetworkCannotExtendExpiryPastUTCMidnight(t *testing.T) {
	s, r, c, a := mapped(t)
	w := r.record.Website
	if _, err := s.Sync(t.Context(), w.ID, w.Site, w.Revision, "algolia"); err != nil {
		t.Fatal(err)
	}
	before := time.Date(2026, 10, 23, 23, 59, 59, 0, time.UTC)
	s.now = func() time.Time { return before }
	a.after = func() { s.now = func() time.Time { return time.Date(2026, 10, 24, 0, 0, 1, 0, time.UTC) } }
	got, err := s.GetContext(t.Context(), w.ID, w.Site, "algolia")
	if err != nil || got.Status != "not_loaded" || got.Window != period(s.now()) || c.calls != 0 {
		t.Fatal(got, err)
	}
}
