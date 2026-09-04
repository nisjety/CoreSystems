package api

import (
	"context"
	"errors"
	"sort"
	"strings"
	"time"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/integration-corev2/internal/providers"
	"github.com/triodelab/integration-corev2/internal/store"
)

// Sync lanes are integration-corev2's per-domain projection of worker health
// onto a connection. One provider connection feeds several independent
// pipelines — the email worker (mail, Teams/Slack/Discord conversations), the
// finspo-core hand-off (SharePoint/OneDrive libraries) and the Data Plane
// document hand-off — and a failure in one of them says nothing about the
// others. `integration_connections.last_sync_status` is a single cell written
// by whichever worker ran last, so surfaces that read it attribute a
// SharePoint failure to the Outlook lane (or vice versa). Consumers should
// read `syncLanes.<lane>` and never `lastSyncStatus`.
const (
	syncLaneMail          = "mail"
	syncLaneCollaboration = "collaboration"
	syncLaneDocuments     = "documents"

	syncLaneSourceEmailWorker = "email-worker"
	syncLaneSourceFinspo      = "finspo-core"
	syncLaneSourceDataPlane   = "data-plane-v2"

	// syncLaneStatus* are the only statuses a lane reports. "pending" means
	// the capability is granted but no worker has attempted the lane yet.
	syncLaneStatusPending   = "pending"
	syncLaneStatusRunning   = "running"
	syncLaneStatusSynced    = "synced"
	syncLaneStatusFailed    = "failed"
	syncLaneStatusCancelled = "cancelled"
)

type syncLane struct {
	Status      string     `json:"status"`
	Source      string     `json:"source"`
	LastSyncAt  *time.Time `json:"lastSyncAt,omitempty"`
	LastError   string     `json:"lastError,omitempty"`
	FailureCode string     `json:"failureCode,omitempty"`
	JobID       string     `json:"jobId,omitempty"`
}

// connectionView is the API shape of a connection: the stored row plus the
// derived sync lanes. Embedding keeps every existing JSON field (and the
// token redaction tags) intact.
type connectionView struct {
	store.Connection
	SyncLanes map[string]syncLane `json:"syncLanes,omitempty"`
}

// emailSyncStateReader is the slice of the repository the lane projection
// needs for the email worker's cursor rows.
type emailSyncStateReader interface {
	GetEmailSyncState(ctx context.Context, connectionID string) (store.EmailSyncState, error)
}

type syncJobLister interface {
	ListSyncJobs(ctx context.Context, filter store.SyncJobFilter) ([]store.SyncJob, error)
}

// connectionViews decorates a list of connections with sync lanes using one
// sync-job query for the whole organization (jobs are grouped per
// connection) plus the email worker's per-lane state rows.
func connectionViews(ctx context.Context, repo store.Repository, organizationID string, connections []store.Connection) []connectionView {
	jobsByConnection := map[string][]store.SyncJob{}
	if lister, ok := repo.(syncJobLister); ok && len(connections) > 0 {
		filter := store.SyncJobFilter{OrganizationID: strings.TrimSpace(organizationID)}
		if filter.OrganizationID == "" && len(connections) == 1 {
			filter.ConnectionID = connections[0].ID
		}
		if jobs, err := lister.ListSyncJobs(ctx, filter); err == nil {
			for _, job := range jobs {
				jobsByConnection[job.ConnectionID] = append(jobsByConnection[job.ConnectionID], job)
			}
		}
	}
	views := make([]connectionView, 0, len(connections))
	for _, connection := range connections {
		views = append(views, connectionView{
			Connection: connection,
			SyncLanes:  connectionSyncLanes(ctx, repo, connection, jobsByConnection[connection.ID]),
		})
	}
	return views
}

func connectionView1(ctx context.Context, repo store.Repository, connection store.Connection) connectionView {
	views := connectionViews(ctx, repo, connection.OrganizationID, []store.Connection{connection})
	if len(views) == 1 {
		return views[0]
	}
	return connectionView{Connection: connection}
}

// connectionSyncLanes derives every lane the connection's grants make
// meaningful. `jobs` must already be limited to this connection.
func connectionSyncLanes(ctx context.Context, states emailSyncStateReader, connection store.Connection, jobs []store.SyncJob) map[string]syncLane {
	lanes := map[string]syncLane{}
	providerKey := providers.NormalizeKey(connection.ProviderKey)

	if connectionGranted(connection, []string{"mail.read", "gmail.read"}, []string{"Mail.Read", "https://www.googleapis.com/auth/gmail.readonly"}) {
		lanes[syncLaneMail] = inboxLane(ctx, states, connection.ID, filterInboxJobs(jobs, "email"))
	}

	switch providerKey {
	case "microsoft":
		if connectionGranted(connection, []string{"teams.messages.read"}, []string{"ChannelMessage.Read.All"}) {
			lanes[syncLaneCollaboration] = inboxLane(ctx, states, connection.ID+":teams", filterInboxJobs(jobs, "teams"))
		}
	case "slack":
		if connectionGranted(connection, []string{"channels.history"}, []string{"channels:history"}) {
			lanes[syncLaneCollaboration] = inboxLane(ctx, states, connection.ID+":slack", filterInboxJobs(jobs, "slack"))
		}
	case "discord":
		if connectionGranted(connection, []string{"messages.read"}, []string{"bot"}) {
			lanes[syncLaneCollaboration] = inboxLane(ctx, states, connection.ID+":discord", filterInboxJobs(jobs, "discord"))
		}
	case "x":
		if connectionGranted(connection, []string{"social.inbox.read"}, []string{"dm.read"}) {
			lanes[syncLaneCollaboration] = inboxLane(ctx, states, connection.ID+":xdm", filterInboxJobs(jobs, "x"))
		}
	}

	documentJobs := filterDocumentJobs(jobs)
	if len(documentJobs) > 0 {
		lanes[syncLaneDocuments] = documentLane(documentJobs)
	} else if providerKey == "microsoft" && connectionGranted(connection, []string{"sharepoint.read"}, []string{"Files.Read.All", "Sites.Read.All"}) {
		lanes[syncLaneDocuments] = syncLane{Status: syncLaneStatusPending, Source: syncLaneSourceFinspo}
	}

	if len(lanes) == 0 {
		return nil
	}
	return lanes
}

// failureCodeNoSourcesRegistered marks a Microsoft sync that cannot run
// because the organization has no SharePoint/OneDrive library registered in
// finspo-core. The sync route refuses with it (409) and the finspo worker
// stamps it on a job that slipped through, so every surface can turn it into
// the same next step: pick a library.
const failureCodeNoSourcesRegistered = "no_sources_registered"

// microsoftSyncNeedsSources reports whether the generic sync for this
// connection must be refused because finspo-core has nothing to sync. It only
// speaks up when it is certain: a missing Finspo client, a non-Microsoft
// provider or a finspo-core error all fall through to the normal queue path.
func microsoftSyncNeedsSources(ctx context.Context, cfg ServerConfig, connection store.Connection) (func(*fiber.Ctx) error, bool) {
	if cfg.Finspo == nil || providers.NormalizeKey(connection.ProviderKey) != "microsoft" {
		return nil, false
	}
	sources, err := cfg.Finspo.ListSources(ctx, connection.OrganizationID, connection.UserID)
	if err != nil || len(sources) > 0 {
		return nil, false
	}
	return func(c *fiber.Ctx) error {
		return apiError(c, fiber.StatusConflict, failureCodeNoSourcesRegistered,
			"No SharePoint or OneDrive library is registered for this organization yet. Pick a library (Knowledge → Add source → SharePoint, or the onboarding connect step) and the first sync starts from there.")
	}, true
}

func connectionGranted(connection store.Connection, capabilities, scopes []string) bool {
	for _, capability := range capabilities {
		if hasCapability(connection.Capabilities, capability) {
			return true
		}
	}
	for _, scope := range scopes {
		for _, granted := range connection.Scopes {
			if strings.EqualFold(strings.TrimSpace(granted), scope) {
				return true
			}
		}
	}
	return false
}

func syncJobHandoffTarget(job store.SyncJob) string {
	if target := stringFromAny(job.Metadata["handoffTarget"]); target != "" {
		return target
	}
	return stringFromAny(job.Checkpoint["handoffTarget"])
}

// filterInboxJobs keeps the manual inbox refreshes the email worker claims for
// one channel. The worker's scheduled polling does not create jobs; its
// outcome lives in email_sync_state and is merged in inboxLane.
func filterInboxJobs(jobs []store.SyncJob, channel string) []store.SyncJob {
	out := []store.SyncJob{}
	for _, job := range jobs {
		if syncJobHandoffTarget(job) != syncLaneSourceEmailWorker {
			continue
		}
		if stringFromAny(job.Metadata["inboxChannel"]) != channel {
			continue
		}
		out = append(out, job)
	}
	return sortJobsNewestFirst(out)
}

// filterDocumentJobs keeps the content hand-offs (finspo-core, Data Plane).
// Inbox refreshes are excluded so a mailbox failure never colours documents.
func filterDocumentJobs(jobs []store.SyncJob) []store.SyncJob {
	out := []store.SyncJob{}
	for _, job := range jobs {
		if syncJobHandoffTarget(job) == syncLaneSourceEmailWorker || strings.TrimSpace(job.Mode) == "inbox" {
			continue
		}
		out = append(out, job)
	}
	return sortJobsNewestFirst(out)
}

func sortJobsNewestFirst(jobs []store.SyncJob) []store.SyncJob {
	sort.SliceStable(jobs, func(i, j int) bool {
		return jobs[i].CreatedAt.After(jobs[j].CreatedAt)
	})
	return jobs
}

func laneStatusFromJob(status string) string {
	switch strings.TrimSpace(status) {
	case "completed":
		return syncLaneStatusSynced
	case "failed":
		return syncLaneStatusFailed
	case "cancelled":
		return syncLaneStatusCancelled
	case "":
		return syncLaneStatusPending
	default:
		// queued, running, waiting_provider, handoff_data_plane: the job has
		// been accepted and a worker owns it.
		return syncLaneStatusRunning
	}
}

func documentLane(jobs []store.SyncJob) syncLane {
	latest := jobs[0]
	lane := syncLane{
		Status:      laneStatusFromJob(latest.Status),
		Source:      firstNonEmpty(syncJobHandoffTarget(latest), syncLaneSourceDataPlane),
		JobID:       latest.ID,
		FailureCode: stringFromAny(latest.Metadata["failureCode"]),
	}
	if lane.Status == syncLaneStatusFailed {
		lane.LastError = stringFromAny(latest.Metadata["failureMessage"])
	}
	// The most recent *successful* completion is the lane's last sync time,
	// even when a later attempt failed — that is what "last synced" means.
	for _, job := range jobs {
		if job.Status == "completed" && job.CompletedAt != nil {
			completed := *job.CompletedAt
			lane.LastSyncAt = &completed
			break
		}
	}
	return lane
}

// inboxLane merges the email worker's scheduled state row with any manual
// refresh jobs for the same channel. A manual job that is still in flight
// wins (the lane is visibly running); otherwise the most recent signal —
// state row or terminal job — decides.
func inboxLane(ctx context.Context, states emailSyncStateReader, stateKey string, jobs []store.SyncJob) syncLane {
	lane := syncLane{Status: syncLaneStatusPending, Source: syncLaneSourceEmailWorker}
	var stateAt time.Time
	if states != nil {
		state, err := states.GetEmailSyncState(ctx, stateKey)
		switch {
		case err == nil:
			stateAt = state.UpdatedAt
			if !state.LastSyncedAt.IsZero() {
				synced := state.LastSyncedAt
				lane.LastSyncAt = &synced
			}
			if strings.TrimSpace(state.LastError) != "" {
				lane.Status = syncLaneStatusFailed
				lane.LastError = strings.TrimSpace(state.LastError)
			} else if !state.LastSyncedAt.IsZero() {
				lane.Status = syncLaneStatusSynced
			}
		case errors.Is(err, store.ErrNotFound):
			// Never attempted: stays pending.
		default:
			// A repository error must not masquerade as a worker failure.
		}
	}
	if len(jobs) == 0 {
		return lane
	}
	latest := jobs[0]
	jobStatus := laneStatusFromJob(latest.Status)
	if jobStatus == syncLaneStatusRunning {
		lane.Status = syncLaneStatusRunning
		lane.JobID = latest.ID
		return lane
	}
	if latest.UpdatedAt.After(stateAt) {
		lane.Status = jobStatus
		lane.JobID = latest.ID
		lane.FailureCode = stringFromAny(latest.Metadata["failureCode"])
		if jobStatus == syncLaneStatusFailed {
			lane.LastError = firstNonEmpty(stringFromAny(latest.Metadata["failureMessage"]), lane.LastError)
		} else {
			lane.LastError = ""
			if jobStatus == syncLaneStatusSynced && latest.CompletedAt != nil {
				completed := *latest.CompletedAt
				lane.LastSyncAt = &completed
			}
		}
	}
	return lane
}
