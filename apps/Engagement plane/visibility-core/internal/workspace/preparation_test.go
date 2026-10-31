package workspace

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/discovery"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
)

type preparedDiscovery struct {
	starts, gets         int
	fail                 bool
	status               string
	startError, getError error
}

func (d *preparedDiscovery) Start(id, site string, rev int64) (discovery.Snapshot, error) {
	d.starts++
	if d.startError != nil {
		return discovery.Snapshot{}, d.startError
	}
	if d.fail {
		return discovery.Snapshot{}, discovery.ErrCooldown
	}
	return discovery.Snapshot{Status: "running"}, nil
}
func (d *preparedDiscovery) Get(id, site string) (discovery.Snapshot, error) {
	d.gets++
	if d.getError != nil {
		return discovery.Snapshot{}, d.getError
	}
	if d.status != "" {
		return discovery.Snapshot{Status: d.status, Reason: "invalid_source"}, nil
	}
	return discovery.Snapshot{Status: "complete"}, nil
}

type preparedMeasurements struct {
	starts, gets int
	result       string
	reason       string
}

func (m *preparedMeasurements) StartPageSpeed(ctx context.Context, site, strategy string) pulse.PageSpeedProjection {
	m.starts++
	return pulse.PageSpeedProjection{Status: "running"}
}
func (m *preparedMeasurements) GetPageSpeed(ctx context.Context, site, strategy string) pulse.PageSpeedProjection {
	m.gets++
	reason := m.reason
	if reason == "" {
		reason = "quota_limited"
	}
	return pulse.PageSpeedProjection{Status: m.result, Reason: reason}
}

func TestPreparationRunnerMissingFailedAndCanceledSources(t *testing.T) {
	registry := workspaceRegistry(t, nil)
	site, _, err := registry.Add(t.Context(), "https://triodelab.no/")
	if err != nil {
		t.Fatal(err)
	}
	for _, step := range []string{"discovery", "pagespeed_mobile", "pagespeed_desktop"} {
		got, err := NewPreparationRunner(registry, nil, nil).Run(t.Context(), site, step)
		if err != nil || got.Status != "unavailable" || got.Reason != "not_configured" {
			t.Fatal(got, err)
		}
	}
	if _, err := NewPreparationRunner(nil, nil, nil).Run(t.Context(), site, "discovery"); err == nil {
		t.Fatal("missing registry accepted")
	}
	for _, d := range []*preparedDiscovery{{startError: errors.New("source failed")}, {getError: errors.New("status failed")}, {status: "failed"}, {status: "idle"}} {
		got, err := NewPreparationRunner(registry, d, nil).Run(t.Context(), site, "discovery")
		if err == nil && got.Status == "complete" {
			t.Fatal("failed discovery fabricated completion")
		}
	}
	for _, tc := range []struct{ status, reason, want string }{{"complete", "", "complete"}, {"idle", "", "unavailable"}, {"failed", "not_configured", "unavailable"}, {"failed", "audit_failed", "failed"}} {
		got, err := NewPreparationRunner(registry, nil, &preparedMeasurements{result: tc.status, reason: tc.reason}).Run(t.Context(), site, "pagespeed_desktop")
		if err != nil || got.Status != tc.want {
			t.Fatal(got, err)
		}
	}
	for _, step := range []string{"discovery", "pagespeed_mobile"} {
		ctx, cancel := context.WithTimeout(t.Context(), 10*time.Millisecond)
		_, err := NewPreparationRunner(registry, &preparedDiscovery{status: "running"}, &preparedMeasurements{result: "running"}).Run(ctx, site, step)
		cancel()
		if !errors.Is(err, context.DeadlineExceeded) {
			t.Fatal("pending source ignored timeout", err)
		}
	}
}

func TestPreparationRunnerUsesExistingBoundedSourcesAndNeverInventsSuccess(t *testing.T) {
	registry := workspaceRegistry(t, nil)
	site, _, err := registry.Add(t.Context(), "https://triodelab.no/")
	if err != nil {
		t.Fatal(err)
	}
	d := &preparedDiscovery{}
	m := &preparedMeasurements{result: "failed"}
	runner := NewPreparationRunner(registry, d, m)
	got, err := runner.Run(t.Context(), site, "discovery")
	if err != nil || got.Status != "complete" || d.starts != 1 || d.gets != 1 {
		t.Fatalf("discovery:%+v %v", got, err)
	}
	got, err = runner.Run(t.Context(), site, "pagespeed_mobile")
	if err != nil || got.Status != "failed" || got.Reason != "quota_limited" || m.starts != 1 || m.gets != 1 {
		t.Fatalf("measurement:%+v %v", got, err)
	}
	d.fail = true
	got, err = runner.Run(t.Context(), site, "discovery")
	if err != nil || got.Status != "unavailable" || got.Reason != "capacity_deferred" {
		t.Fatalf("capacity:%+v %v", got, err)
	}
	if _, err = runner.Run(t.Context(), site, "untrusted"); err == nil {
		t.Fatal("unknown step accepted")
	}
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	before := m.starts
	_, err = runner.Run(ctx, site, "pagespeed_desktop")
	if !errors.Is(err, context.Canceled) || m.starts != before {
		t.Fatal("canceled runner started provider")
	}
	changed, err := registry.Change(t.Context(), site.ID, "https://other.no/", site.Revision)
	if err != nil || changed.Revision == site.Revision {
		t.Fatal(err)
	}
	before = m.starts
	_, err = runner.Run(t.Context(), site, "pagespeed_desktop")
	if err == nil || m.starts != before {
		t.Fatal("old website initiated provider work")
	}
}

type cachedPreparationMeasurement struct {
	status string
	reads  int
}

func (m *cachedPreparationMeasurement) StartPageSpeed(context.Context, string, string) pulse.PageSpeedProjection {
	return pulse.PageSpeedProjection{Status: "complete", Cache: &pulse.PageSpeedCache{RefreshStatus: "queued"}}
}
func (m *cachedPreparationMeasurement) GetPageSpeed(context.Context, string, string) pulse.PageSpeedProjection {
	m.reads++
	return pulse.PageSpeedProjection{Status: "complete", Cache: &pulse.PageSpeedCache{RefreshStatus: m.status, RefreshReason: "quota_limited"}}
}
func TestPreparationWaitsForRefreshInsteadOfTreatingOldCachedDataAsNewSuccess(t *testing.T) {
	registry := workspaceRegistry(t, nil)
	w, _, err := registry.Add(t.Context(), "https://triodelab.no/")
	if err != nil {
		t.Fatal(err)
	}
	m := &cachedPreparationMeasurement{status: "failed"}
	result, err := NewPreparationRunner(registry, nil, m).Run(t.Context(), w, "pagespeed_mobile")
	if err != nil || result.Status != "failed" || m.reads != 1 {
		t.Fatal(result, err, m.reads)
	}
	m = &cachedPreparationMeasurement{status: "running"}
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Millisecond)
	defer cancel()
	_, err = NewPreparationRunner(registry, nil, m).Run(ctx, w, "pagespeed_desktop")
	if !errors.Is(err, context.DeadlineExceeded) || m.reads != 1 {
		t.Fatal(err, m.reads)
	}
}
