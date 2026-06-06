package workers

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"strings"
	"time"

	"github.com/rs/zerolog"

	"github.com/triodelab/integration-corev2/internal/handoff"
	"github.com/triodelab/integration-corev2/internal/store"
)

const (
	finspoConsumer = "finspo-core"
	microsoftKey   = "microsoft"
)

type IntegrationSyncClient interface {
	ClaimSyncJob(context.Context, handoff.SyncClaimRequest) (store.SyncJob, error)
	UpdateSyncProgress(context.Context, string, handoff.SyncProgressRequest) (store.SyncJob, error)
}

type FinspoSourceClient interface {
	EnsureSource(context.Context, string, string, handoff.FinspoSourceRequest) (handoff.FinspoSource, error)
	SyncSource(context.Context, string, string, string) (handoff.FinspoSyncResult, error)
}

type FinspoWorker struct {
	Integration  IntegrationSyncClient
	Finspo       FinspoSourceClient
	PollInterval time.Duration
	Logger       *zerolog.Logger
}

func (w FinspoWorker) Run(ctx context.Context) error {
	interval := w.PollInterval
	if interval <= 0 {
		interval = 5 * time.Second
	}
	ticker := time.NewTicker(interval)
	defer ticker.Stop()
	for {
		processed, err := w.RunOnce(ctx)
		if err != nil {
			w.logWarn(err, "finspo worker iteration failed")
		}
		if processed {
			continue
		}
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-ticker.C:
		}
	}
}

func (w FinspoWorker) RunOnce(ctx context.Context) (bool, error) {
	if w.Integration == nil || w.Finspo == nil {
		return false, handoff.ErrNotConfigured
	}
	job, err := w.Integration.ClaimSyncJob(ctx, handoff.SyncClaimRequest{
		Consumer:    finspoConsumer,
		Target:      finspoConsumer,
		ProviderKey: microsoftKey,
	})
	if err != nil {
		var serviceErr *handoff.ServiceHTTPError
		if errors.As(err, &serviceErr) && serviceErr.StatusCode == http.StatusNotFound {
			return false, nil
		}
		return false, err
	}
	if err := w.process(ctx, job); err != nil {
		return true, err
	}
	return true, nil
}

func (w FinspoWorker) process(ctx context.Context, job store.SyncJob) error {
	sourceRequest, sourceRef, err := finspoSourceRequest(job)
	if err != nil {
		return w.failJob(ctx, job, err)
	}
	source, err := w.Finspo.EnsureSource(ctx, job.OrganizationID, job.UserID, sourceRequest)
	if err != nil {
		return w.failJob(ctx, job, fmt.Errorf("ensure Finspo source: %w", err))
	}
	sourceID := firstNonEmpty(source.ID, sourceRef.SourceID)
	if sourceID == "" {
		return w.failJob(ctx, job, errors.New("Finspo source response did not include a source id"))
	}
	syncResult, err := w.Finspo.SyncSource(ctx, job.OrganizationID, job.UserID, sourceID)
	if err != nil {
		return w.failJob(ctx, job, fmt.Errorf("sync Finspo source: %w", err))
	}
	sourceRef.Provider = finspoConsumer
	sourceRef.SourceID = sourceID
	sourceRef.Status = firstNonEmpty(syncResult.Status, source.Status, "queued")
	if sourceRef.Title == "" {
		sourceRef.Title = firstNonEmpty(source.DriveName, sourceRequest.DriveName, "Microsoft 365 source")
	}
	_, err = w.Integration.UpdateSyncProgress(ctx, job.ID, handoff.SyncProgressRequest{
		Consumer: finspoConsumer,
		Status:   "completed",
		Message:  "Finspo source sync was accepted.",
		Checkpoint: map[string]any{
			"finspoSourceId":   sourceID,
			"finspoSyncJobId":  syncResult.JobID,
			"finspoSyncStatus": sourceRef.Status,
		},
		Sources: []handoff.SyncSourceRef{sourceRef},
	})
	if err != nil {
		return fmt.Errorf("complete Finspo sync job: %w", err)
	}
	return nil
}

func (w FinspoWorker) failJob(ctx context.Context, job store.SyncJob, failure error) error {
	_, progressErr := w.Integration.UpdateSyncProgress(ctx, job.ID, handoff.SyncProgressRequest{
		Consumer: finspoConsumer,
		Status:   "failed",
		Message:  failure.Error(),
		Metadata: map[string]any{
			"failure": "finspo_worker",
		},
	})
	if progressErr != nil {
		return fmt.Errorf("%w; failed to mark sync job failed: %v", failure, progressErr)
	}
	return failure
}

func (w FinspoWorker) logWarn(err error, message string) {
	if w.Logger == nil || err == nil {
		return
	}
	w.Logger.Warn().Err(err).Msg(message)
}

func finspoSourceRequest(job store.SyncJob) (handoff.FinspoSourceRequest, handoff.SyncSourceRef, error) {
	lookup := func(keys ...string) string {
		for _, key := range keys {
			if value := stringFromMap(job.Checkpoint, key); value != "" {
				return value
			}
			if value := stringFromMap(job.Metadata, key); value != "" {
				return value
			}
		}
		return ""
	}
	siteID := lookup("site_id", "siteId", "sharepointSiteId", "sharepoint_site_id")
	driveID := lookup("drive_id", "driveId", "sharepointDriveId", "sharepoint_drive_id")
	if siteID == "" || driveID == "" {
		return handoff.FinspoSourceRequest{}, handoff.SyncSourceRef{}, errors.New("Finspo handoff requires SharePoint site_id and drive_id")
	}
	request := handoff.FinspoSourceRequest{
		SiteID:     siteID,
		DriveID:    driveID,
		TenantID:   lookup("tenant_id", "tenantId"),
		SiteWebURL: lookup("site_web_url", "siteWebUrl", "url"),
		DriveName:  lookup("drive_name", "driveName", "title"),
		DriveType:  lookup("drive_type", "driveType"),
	}
	ref := handoff.SyncSourceRef{
		Provider:   finspoConsumer,
		Type:       "sharepoint_drive",
		ExternalID: driveID,
		Status:     "queued",
		Title:      request.DriveName,
		URL:        request.SiteWebURL,
	}
	return request, ref, nil
}

func stringFromMap(input map[string]any, key string) string {
	if len(input) == 0 || strings.TrimSpace(key) == "" {
		return ""
	}
	switch value := input[key].(type) {
	case string:
		return strings.TrimSpace(value)
	default:
		return ""
	}
}

func firstNonEmpty(values ...string) string {
	for _, value := range values {
		if trimmed := strings.TrimSpace(value); trimmed != "" {
			return trimmed
		}
	}
	return ""
}
