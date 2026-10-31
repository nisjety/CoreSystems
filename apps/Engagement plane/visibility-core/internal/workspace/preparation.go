package workspace

import (
	"context"
	"errors"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/discovery"
	"github.com/triodelab/engagement-plane/visibility-core/internal/preparation"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type preparationDiscovery interface {
	Start(string, string, int64) (discovery.Snapshot, error)
	Get(string, string) (discovery.Snapshot, error)
}
type preparationMeasurements interface {
	StartPageSpeed(context.Context, string, string) pulse.PageSpeedProjection
	GetPageSpeed(context.Context, string, string) pulse.PageSpeedProjection
}
type PreparationRunner struct {
	registry     *websites.Service
	discovery    preparationDiscovery
	measurements preparationMeasurements
}

func NewPreparationRunner(registry *websites.Service, d preparationDiscovery, m preparationMeasurements) *PreparationRunner {
	return &PreparationRunner{registry: registry, discovery: d, measurements: m}
}
func (r *PreparationRunner) current(ctx context.Context, w websites.Website) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if r.registry == nil {
		return preparation.ErrInvalid
	}
	record, err := r.registry.Resolve(w.ID, w.Site)
	if err != nil || record.Website.Revision != w.Revision {
		return preparation.ErrConflict
	}
	return nil
}
func (r *PreparationRunner) Run(ctx context.Context, w websites.Website, step string) (preparation.StepResult, error) {
	if err := r.current(ctx, w); err != nil {
		return preparation.StepResult{}, err
	}
	switch step {
	case "discovery":
		return r.discover(ctx, w)
	case "pagespeed_mobile":
		return r.measure(ctx, w, "mobile")
	case "pagespeed_desktop":
		return r.measure(ctx, w, "desktop")
	default:
		return preparation.StepResult{}, preparation.ErrInvalid
	}
}
func (r *PreparationRunner) discover(ctx context.Context, w websites.Website) (preparation.StepResult, error) {
	if r.discovery == nil {
		return unavailablePreparation("not_configured"), nil
	}
	v, err := r.discovery.Start(w.ID, w.Site, w.Revision)
	if errors.Is(err, discovery.ErrCooldown) {
		return unavailablePreparation("capacity_deferred"), nil
	}
	if err != nil {
		return unavailablePreparation("source_unavailable"), err
	}
	for {
		if v.Status == "complete" {
			return preparation.StepResult{Status: "complete"}, nil
		}
		if v.Status == "failed" {
			return failedPreparation(v.Reason), nil
		}
		if v.Status != "running" {
			return unavailablePreparation("source_unavailable"), nil
		}
		if err = r.current(ctx, w); err != nil {
			return preparation.StepResult{}, err
		}
		v, err = r.discovery.Get(w.ID, w.Site)
		if err != nil {
			return unavailablePreparation("source_unavailable"), err
		}
		if v.Status != "running" {
			continue
		}
		if err = waitPreparation(ctx); err != nil {
			return preparation.StepResult{}, err
		}
	}
}
func (r *PreparationRunner) measure(ctx context.Context, w websites.Website, strategy string) (preparation.StepResult, error) {
	if r.measurements == nil {
		return unavailablePreparation("not_configured"), nil
	}
	v := r.measurements.StartPageSpeed(ctx, w.Site, strategy)
	for {
		if err := r.current(ctx, w); err != nil {
			return preparation.StepResult{}, err
		}
		pending := v.Status == "running" || v.Cache != nil && (v.Cache.RefreshStatus == "queued" || v.Cache.RefreshStatus == "running")
		if v.Cache != nil && v.Cache.RefreshStatus == "failed" {
			return failedPreparation(v.Cache.RefreshReason), nil
		}
		if !pending {
			if v.Status == "complete" {
				return preparation.StepResult{Status: "complete"}, nil
			}
			if v.Status == "failed" {
				return failedPreparation(v.Reason), nil
			}
			return unavailablePreparation("source_unavailable"), nil
		}
		v = r.measurements.GetPageSpeed(ctx, w.Site, strategy)
		if v.Status == "running" || v.Cache != nil && (v.Cache.RefreshStatus == "queued" || v.Cache.RefreshStatus == "running") {
			if err := waitPreparation(ctx); err != nil {
				return preparation.StepResult{}, err
			}
		}
	}

}
func waitPreparation(ctx context.Context) error {
	timer := time.NewTimer(time.Second)
	defer timer.Stop()
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-timer.C:
		return nil
	}
}
func unavailablePreparation(reason string) preparation.StepResult {
	return preparation.StepResult{Status: "unavailable", Reason: reason}
}
func failedPreparation(reason string) preparation.StepResult {
	if reason == "not_configured" {
		return unavailablePreparation(reason)
	}
	if reason != "quota_limited" {
		reason = "source_unavailable"
	}
	return preparation.StepResult{Status: "failed", Reason: reason}
}
