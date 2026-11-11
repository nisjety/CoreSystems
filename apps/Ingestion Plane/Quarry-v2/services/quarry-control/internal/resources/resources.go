// Package resources mounts REST routes for control-plane resources.
// Each resource gets list / get / create / history (where applicable).
package resources

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/url"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/audit"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/notify"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// JobsOption customises MountJobs.
type JobsOption func(*jobsConfig)

// jobsConfig is what the JobsOptions set. The zero value is the behaviour
// before either option existed, except that a job asking to ignore robots.txt
// is refused (no audit sink, no override).
type jobsConfig struct {
	adm         Admission
	robotsAudit RobotsAuditor
}

// WithAdmission enables the job-admission policy (per-organization caps, a
// bounded backlog, the claim endpoint). Without it control behaves as before.
func WithAdmission(a Admission) JobsOption {
	return func(into *jobsConfig) { into.adm = a }
}

// WithRobotsAudit gives POST /v1/jobs the audit sink that a robots.txt
// override must be written to before the job exists (WS14.S7b). Without it, or
// with a nil sink, every job that asks to ignore robots.txt answers 503 and
// nothing is stored; every other job is unaffected.
func WithRobotsAudit(a RobotsAuditor) JobsOption {
	return func(into *jobsConfig) { into.robotsAudit = a }
}

func MountJobs(r chi.Router, db store.DB, opts ...JobsOption) {
	var cfg jobsConfig
	for _, opt := range opts {
		opt(&cfg)
	}
	adm := cfg.adm
	// Dispatcher-only: take leased work instead of reading a window. Mounted
	// under the same HMAC group as every other /v1 route; 404 while admission is
	// off so a newer dispatcher falls back to listing.
	r.Post("/v1/internal/jobs/claim", claimJobs(db, adm))
	// Job control (WS15.L2): the orchestrator control loop reads the pending
	// requests, acknowledges them, lists running jobs and finalizes ended ones.
	r.Get("/v1/internal/jobs/pending-control", pendingControl(db))
	r.Post("/v1/internal/jobs/{id}/control-ack", controlAck(db))
	r.Get("/v1/internal/jobs/running", runningJobs(db))
	r.Post("/v1/internal/jobs/{id}/finalize", finalizeJob(db))
	r.Route("/v1/jobs", func(r chi.Router) {
		r.Get("/", listJobs(db))
		r.Post("/", createJob(db, adm, cfg.robotsAudit))
		r.Get("/{id}", getJob(db))
		r.Put("/{id}", updateJob(db))
		r.Get("/{id}/events", jobEvents(db))
		r.Get("/{id}/history", jobHistory(db))
		r.Get("/ongoing", jobsOngoing(db))
		r.Get("/{id}/summary", jobSummary(db))
		r.Get("/{id}/errors", jobErrors(db))
		r.Get("/{id}/results", jobResults(db))
		r.Post("/{id}/control", controlJob(db))
	})
}

func MountStores(r chi.Router, db store.DB) {
	mountSimple[store.NamedStore](r, "/v1/stores", db.Stores())
}
func MountArtifacts(r chi.Router, db store.DB) {
	mountSimple[store.Artifact](r, "/v1/artifacts", db.Artifacts())
}
func MountProfiles(r chi.Router, db store.DB) {
	mountSimple[store.BrowserProfile](r, "/v1/profiles", db.Profiles())
}
func MountEvents(r chi.Router, db store.DB, apiKey string, sink notify.Sink, opts ...NoticeOption) {
	// WS15.S4d: with WithScheduleNotices, a failed or timed-out fire of a schedule
	// notifies the schedule's creator. Without it nothing is sent.
	notices := noticeConfigOf(opts)
	// GET /v1/runs/{id}/events backs quarry-edge's list_run_events fallback
	// (crates/quarry-edge/src/resource_routes.rs), which deserializes every
	// item strictly as quarry_core::job_history::JobHistoryEvent — org_id
	// and kind are required fields that a bare quarrycontracts.Event has no
	// equivalent for. Resolve the owning job via GetByRunID once (every
	// event in a run shares one org_id/kind) and translate each row through
	// toJobHistoryEvent (event_wire.go) rather than serializing the legacy
	// Event shape directly.
	r.Get("/v1/runs/{id}/events", func(w http.ResponseWriter, r *http.Request) {
		runID := quarrycontracts.ID(chi.URLParam(r, "id"))
		if err := runID.MustKind(quarrycontracts.KindRun); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		owner, ok := db.Jobs().GetByRunID(runID)
		if !ok {
			httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "no job dispatched this run_id", nil)
			return
		}
		after, _ := strconv.ParseUint(r.URL.Query().Get("after_seq"), 10, 64)
		limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
		if limit <= 0 {
			limit = 100
		}
		evts := db.Events().ForRun(runID, after, limit)
		wire := make([]jobHistoryEventWire, 0, len(evts))
		for _, evt := range evts {
			wire = append(wire, toJobHistoryEvent(evt, owner))
		}
		httpx.WriteJSON(w, r, http.StatusOK, wire)
	})

	// Append-only event log (batch). Runtime + orchestrator post here.
	// Protected by bearer auth when apiKey is non-empty.
	r.With(httpx.BearerAuth(apiKey)).Post("/v1/runs/{id}/events", func(w http.ResponseWriter, r *http.Request) {
		runID := quarrycontracts.ID(chi.URLParam(r, "id"))
		if err := runID.MustKind(quarrycontracts.KindRun); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		// Cap the body at 1 MB. Without this a malicious caller can
		// stream an arbitrarily large JSON array and pin a goroutine
		// in `Decode`. Real event batches are tens of KB at most.
		const maxEventBatchBytes = 1 << 20
		r.Body = http.MaxBytesReader(w, r.Body, maxEventBatchBytes)
		var batch []quarrycontracts.Event
		if err := json.NewDecoder(r.Body).Decode(&batch); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		if len(batch) == 0 {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "empty batch", nil)
			return
		}
		// Callers do not share one sequence contract: workflow activities
		// usually emit seq=0, while quarry-edge's runtime sink emits a
		// process-wide counter. The control plane owns durable per-run
		// ordering, so never trust caller seq here: each event takes the
		// run's next seq in the same transaction as its insert (D2-016), so
		// two batches of one run in flight at once cannot collide on a seq,
		// and an event is never dropped as a replay because it lost one.
		// The org that owns the run decides which subscriptions may see its events
		// (INNH-5). An unknown run resolves to "", and an empty org fans out to nobody.
		fanoutOrg := ""
		if owner, found := db.Jobs().GetByRunID(runID); found {
			fanoutOrg = owner.OrgID
		}
		for i := range batch {
			if batch[i].Type == "" {
				httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "event.type required", map[string]any{"index": i})
				return
			}
			if batch[i].EventID == "" {
				batch[i].EventID = quarrycontracts.NewID(quarrycontracts.KindEvent)
			}
			if batch[i].Timestamp.IsZero() {
				batch[i].Timestamp = time.Now().UTC()
			}
			rid := runID
			batch[i].RunID = &rid
			seq, err := db.Events().AppendNext(batch[i])
			if err == nil {
				batch[i].Seq = seq
			}
			if err != nil {
				if errors.Is(err, store.ErrConflict) {
					// Idempotent replay: a publisher may retry a batch
					// after a partial success. Keep accepting the rest
					// instead of returning 500 and trapping the publisher
					// in an infinite retry loop.
					continue
				}
				httpx.WriteErr(w, r, quarrycontracts.CodeInternal, err.Error(), map[string]any{"index": i})
				return
			}
			markJobTerminalIfNeeded(db, batch[i])
			if batch[i].Type == quarrycontracts.EvtChangeDetected {
				// E3.5-7b: a change takes its subscriptions' notifyOnChange filter on this path too, as it does
				// on the signed change receiver; the shared dedupe key keeps one delivery per subscription.
				fanoutChangeWebhooks(db, batch[i], fanoutOrg, eventChangeNotice(batch[i]), changeNotifyFilterOf)
			} else {
				fanoutWebhooks(db, batch[i], fanoutOrg)
			}
			fanoutJobWebhook(db, batch[i], fanoutOrg)
			notifyOnChange(sink, batch[i])
			notifyOnScheduleRun(db, notices.scheduleSink, batch[i])
		}
		httpx.WriteJSON(w, r, http.StatusCreated, map[string]any{"accepted": len(batch)})
	})
}

// notifyOnChange delivers the ONE in-product notification for a detected
// change. The recipient is the schedule's creator (payload.created_by, set by
// ChangeMonitorWF). Best-effort + synchronous so it isn't lost on shutdown,
// but it never fails the event append: a notification-core outage must not
// block change tracking or webhook fanout. Idempotent on event_id so the
// idempotent-replay path (ErrConflict → continue) never double-notifies.
func notifyOnChange(sink notify.Sink, ev quarrycontracts.Event) {
	if sink == nil || ev.Type != quarrycontracts.EvtChangeDetected {
		return
	}
	recipient, _ := ev.Payload["created_by"].(string)
	if strings.TrimSpace(recipient) == "" {
		// No creator recorded (legacy schedule / non-user path) — honest skip.
		return
	}
	orgID, _ := ev.Payload["org_id"].(string)
	if strings.TrimSpace(orgID) == "" {
		// The organization scopes the notification; without it there is nothing to scope to.
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := sink.NotifyChange(ctx, orgID, recipient, string(ev.EventID), ev.Payload); err != nil {
		log.Warn().Err(err).Str("event_id", string(ev.EventID)).Msg("change notification delivery failed")
	}
}

func markJobTerminalIfNeeded(db store.DB, ev quarrycontracts.Event) {
	if ev.JobID == nil {
		return
	}
	status, ok := jobStatusForEvent(ev.Type)
	if !ok {
		return
	}
	job, exists := db.Jobs().Get(*ev.JobID)
	if !exists {
		return
	}
	// A job that ended stays ended: a late event (a resume after a cancel, a
	// page after a timeout) must not move it back to a live status.
	if store.IsTerminalJobStatus(job.Status) {
		return
	}
	job.Status = status
	if store.IsTerminalJobStatus(status) {
		at := ev.Timestamp
		if at.IsZero() {
			at = time.Now()
		}
		if job.FinishedAt == nil {
			ms := at.UnixMilli()
			job.FinishedAt = &ms
		}
		job.TerminalReason = string(ev.Type)
	}
	if statistics := terminalStatistics(ev.Payload); statistics != nil {
		job.Statistics = statistics
	}
	_ = db.Jobs().Update(job)
}

// maxStatisticsBytes bounds what a terminal event may attach to a job. A
// crawl's statistics are a few dozen counters from closed vocabularies; anything
// larger is not what the orchestrator sends and is dropped rather than stored.
const maxStatisticsBytes = 4096

// terminalStatistics returns the "statistics" object of a terminal event's
// payload, or nil when it is absent, not an object, not encodable or oversize.
func terminalStatistics(payload map[string]any) map[string]any {
	statistics, ok := payload["statistics"].(map[string]any)
	if !ok || len(statistics) == 0 {
		return nil
	}
	encoded, err := json.Marshal(statistics)
	if err != nil || len(encoded) > maxStatisticsBytes {
		return nil
	}
	return statistics
}

func jobStatusForEvent(eventType quarrycontracts.EventType) (string, bool) {
	switch eventType {
	case quarrycontracts.EvtRunCompleted:
		return "completed", true
	case quarrycontracts.EvtRunFailed:
		return "failed", true
	case quarrycontracts.EvtRunCancelled:
		return "cancelled", true
	case quarrycontracts.EvtRunPaused:
		return "paused", true
	case quarrycontracts.EvtRunResumed:
		return "running", true
	case quarrycontracts.EventType("run_timed_out"):
		return "timed_out", true
	case quarrycontracts.EventType("schedule_skipped"):
		return "skipped", true
	default:
		return "", false
	}
}

func mountSimple[T any](r chi.Router, path string, s store.ResourceStore[T]) {
	r.Route(path, func(r chi.Router) {
		r.Get("/", func(w http.ResponseWriter, r *http.Request) {
			limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
			if limit <= 0 {
				limit = 50
			}
			items, _ := s.List(limit, r.URL.Query().Get("cursor"))
			httpx.WriteJSON(w, r, http.StatusOK, items)
		})
		r.Post("/", func(w http.ResponseWriter, r *http.Request) {
			var v T
			if err := json.NewDecoder(r.Body).Decode(&v); err != nil {
				httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
				return
			}
			if vv, ok := any(&v).(interface{ Validate() error }); ok {
				if err := vv.Validate(); err != nil {
					httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
					return
				}
			}
			if err := s.Create(v); err != nil {
				httpx.WriteErr(w, r, quarrycontracts.CodeConflict, err.Error(), nil)
				return
			}
			httpx.WriteJSON(w, r, http.StatusCreated, v)
		})
		r.Get("/{id}", func(w http.ResponseWriter, r *http.Request) {
			id := quarrycontracts.ID(chi.URLParam(r, "id"))
			v, ok := s.Get(id)
			if !ok {
				httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": id})
				return
			}
			httpx.WriteJSON(w, r, http.StatusOK, v)
		})
	})
}

// listJobs backs GET /v1/jobs. UNSCOPED BY DESIGN: quarry-orchestrator's
// jobs dispatcher (services/quarry-orchestrator/internal/jobs/dispatcher.go
// listJobs) polls this exact endpoint with NO org_id — it must see every
// tenant's `accepted` jobs to start the matching Temporal workflow, the
// same way the schedules reconciler polls GET /v1/schedules unscoped.
// There is no tenant-facing caller: quarry-edge never forwards to this
// bare path (only to the org-scoped GET /v1/{kind}/jobs — see
// MountJobsByKind), and the GraphQL `jobs` resolver is an inert stub that
// never reaches control. Scoping this endpoint would silently stop every
// job from ever leaving "accepted".
func listJobs(db store.DB) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
		if limit <= 0 {
			limit = 50
		}
		items, _ := db.Jobs().List(limit, r.URL.Query().Get("cursor"))
		for i := range items {
			items[i] = withoutWebhookSecret(items[i])
		}
		httpx.WriteJSON(w, r, http.StatusOK, items)
	}
}

// createJob backs POST /v1/jobs. org_id is read from the edge-verified
// `?org_id` query param (never the request body) and stamped onto the
// job — mirrors createSourceHandler's guard, which rejects an empty org
// rather than let a job land unattributed to any tenant.
//
// WS14.S7b: a job whose params ask to ignore robots.txt is audited
// (crawl.robots_ignored) BEFORE it is stored and acknowledged. No audit row, no
// job: a missing audit sink or an audit-core failure answers 503 and stores
// nothing. Jobs that do not ask to ignore robots.txt never reach the sink.
func createJob(db store.DB, adm Admission, robotsAudit RobotsAuditor) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		org := orgFromQuery(r)
		if org == "" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
			return
		}
		var in createJobInput
		if err := json.NewDecoder(r.Body).Decode(&in); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		policy, err := resolveJobPolicy(in)
		if err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		origin, err := resolveJobOrigin(in.Origin)
		if err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		robotsIgnore, err := robotsIgnoreFromParams(in.Params)
		if err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		// Honor `Idempotency-Key` header: if a job with the same key
		// already exists, return it (200 OK) rather than creating a
		// duplicate (201). This lets verevon safely retry the POST on
		// a network hiccup, and turns a user's double-click into a
		// single crawl. Keys longer than 128 chars are rejected to
		// bound the index.
		idemKey := strings.TrimSpace(r.Header.Get("Idempotency-Key"))
		if len(idemKey) > 128 {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "idempotency key too long (max 128)", nil)
			return
		}
		if idemKey != "" {
			if existing, ok := db.Jobs().FindByIdempotencyKey(org, idemKey); ok {
				httpx.WriteJSON(w, r, http.StatusOK, withoutWebhookSecret(existing))
				return
			}
		}
		// Bound the backlog before anything is stored: a refused submission
		// leaves no row. Idempotent retries returned above are not affected.
		if details, full := adm.backlogFull(db, org, in.Kind, in.Params, time.Now()); full {
			w.Header().Set("Retry-After", strconv.Itoa(retryAfterSecs))
			httpx.WriteErr(w, r, quarrycontracts.CodeRateLimited,
				"this organization has too many jobs waiting", details)
			return
		}
		// W3-E3.5-3b: vet and normalise the per-job webhook before anything is stored.
		webhookKey, refused := prepareJobWebhook(r.Context(), in.Params)
		if refused != nil {
			refused(w, r)
			return
		}
		job := store.Job{
			ID:        quarrycontracts.NewID(quarrycontracts.KindJob),
			OrgID:     org,
			Kind:      in.Kind,
			Status:    "accepted",
			Policy:    policy,
			Params:    in.Params,
			CreatedAt: time.Now().UnixMilli(),
			Origin:    origin,
		}
		if idemKey != "" {
			k := idemKey
			job.IdempotencyKey = &k
		}
		if robotsIgnore != nil {
			if err := recordRobotsIgnored(r.Context(), robotsAudit, job, *robotsIgnore, httpx.RequestIDOf(r.Context())); err != nil {
				log.Error().Err(err).Str("org_id", org).Str("job_id", string(job.ID)).
					Msg("createJob: robots.txt override not audited; job refused")
				httpx.WriteErr(w, r, quarrycontracts.CodeRuntimeNotReady,
					"the robots.txt override could not be audited, so no job was created",
					map[string]any{"reason": "robots_override_audit_unavailable"})
				return
			}
		}
		if err := db.Jobs().Create(job); err != nil {
			// Race: another request with the same idempotency key
			// landed between our lookup and our Create. Re-fetch and
			// return that one.
			if idemKey != "" {
				if existing, ok := db.Jobs().FindByIdempotencyKey(org, idemKey); ok {
					httpx.WriteJSON(w, r, http.StatusOK, existing)
					return
				}
			}
			httpx.WriteErr(w, r, quarrycontracts.CodeConflict, err.Error(), nil)
			return
		}
		created := jobCreateResponse{Job: job}
		if hook, ok := JobWebhookFromParams(job.Params); ok && webhookKey != nil {
			// The secret is derived from the control key and this (org, job) pair; it is not
			// stored and no other response carries it.
			if secret, err := JobWebhookSecret(webhookKey, org, string(job.ID)); err == nil {
				created.Webhook = &jobCreateWebhook{URL: hook.URL, Events: hook.Events, Secret: secret}
			}
		}
		httpx.WriteJSON(w, r, http.StatusCreated, created)
	}
}

// jobWebhookGetenv and jobWebhookVetter are the switch reader and the SSRF vet the create path
// uses; a test swaps them.
var (
	jobWebhookGetenv            = os.Getenv
	jobWebhookVetter HostVetter = DefaultHostVetter
)

// prepareJobWebhook handles params.webhook of a job create (W3-E3.5-3b). Off
// (QUARRY_CONTROL__JOB_WEBHOOK unset) the key is dropped silently: the edge flag is the
// user-facing refusal and nothing may be delivered. On, the value is vetted and normalised
// (NormalizeJobWebhook: shape, https, egress guard on every resolved address, closed events,
// ZDR refused) and replaced by its stored form. The returned key is the control signing key,
// nil when the job has no webhook. refused, when set, writes the typed refusal: a 400 naming the
// field and reason, or 503 job_webhook_unconfigured when the key is missing or short, never an
// unsigned hook.
func prepareJobWebhook(ctx context.Context, params map[string]any) (key []byte, refused func(http.ResponseWriter, *http.Request)) {
	raw, present := params[JobWebhookParamKey]
	if !present {
		return nil, nil
	}
	if !JobWebhooksEnabled(jobWebhookGetenv) {
		delete(params, JobWebhookParamKey)
		return nil, nil
	}
	hook, field, reason, msg := NormalizeJobWebhook(ctx, raw, jobParamsZdr(params), jobWebhookVetter)
	if reason != "" {
		return nil, func(w http.ResponseWriter, r *http.Request) {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, msg, map[string]any{"field": field, "reason": reason})
		}
	}
	key, err := JobWebhookKey(jobWebhookGetenv)
	if err != nil {
		return nil, func(w http.ResponseWriter, r *http.Request) {
			httpx.WriteErr(w, r, quarrycontracts.CodeRuntimeNotReady,
				"per-job webhooks are not configured, so no job was created",
				map[string]any{"reason": "job_webhook_unconfigured"})
		}
	}
	params[JobWebhookParamKey] = hook.ParamsValue()
	return key, nil
}

// RobotsAuditor is the audit sink a robots.txt override is written to before
// the job exists. *audit.Client satisfies it.
type RobotsAuditor interface {
	RecordAudit(ctx context.Context, event audit.Event) error
}

// RobotsIgnoredEvent is the audit event type written for every job that
// ignores robots.txt.
const RobotsIgnoredEvent = "crawl.robots_ignored"

// robotsAuditTimeout bounds the audit write so a slow audit-core cannot hold
// the create request open; a timeout is a refusal like any other failure.
const robotsAuditTimeout = 5 * time.Second

// robotsIgnoreRequest is what a job asking to ignore robots.txt carries.
type robotsIgnoreRequest struct {
	UserID    string
	Authority string
	Rev       any
	SeedHost  string
}

// robotsIgnoreFromParams reports whether the job asks to ignore robots.txt.
// Two shapes count, so neither can slip past the audit:
//
//   - params.robots == "ignore", the string the edge stamps today and the one
//     the orchestrator honours (workflows/inputs.go stringFromParam);
//   - params.robots == {mode:"ignore", authority, rev}, the object WS14.S7a
//     stamps for a granted override.
//
// The mode compares case-insensitively, so "IGNORE" is audited (and refused
// without a sink) rather than slipping through as an unknown value. Any other
// mode, an absent key or null is not an override (nil, nil). An override with
// no params.user_id is a 400: the audit row must name the person.
func robotsIgnoreFromParams(params map[string]any) (*robotsIgnoreRequest, error) {
	raw, present := params["robots"]
	if !present || raw == nil {
		return nil, nil
	}
	var mode, authority string
	var rev any
	switch robots := raw.(type) {
	case string:
		mode = robots
	case map[string]any:
		mode, _ = robots["mode"].(string)
		authority, _ = robots["authority"].(string)
		rev = robots["rev"]
	default:
		return nil, nil
	}
	if !strings.EqualFold(strings.TrimSpace(mode), "ignore") {
		return nil, nil
	}
	userID, _ := params["user_id"].(string)
	userID = strings.TrimSpace(userID)
	if userID == "" {
		return nil, fmt.Errorf("params.user_id is required when params.robots asks to ignore robots.txt")
	}
	return &robotsIgnoreRequest{
		UserID:    userID,
		Authority: strings.TrimSpace(authority),
		Rev:       rev,
		SeedHost:  seedHostFromParams(params),
	}, nil
}

// seedHostFromParams is the lower-cased host of params.url (or seed_url), or
// "" when neither parses. Only the host is audited, never the path or query.
func seedHostFromParams(params map[string]any) string {
	for _, key := range []string{"url", "seed_url"} {
		raw, _ := params[key].(string)
		raw = strings.TrimSpace(raw)
		if raw == "" {
			continue
		}
		if u, err := url.Parse(raw); err == nil && u.Hostname() != "" {
			return strings.ToLower(u.Hostname())
		}
	}
	return ""
}

// recordRobotsIgnored writes the one crawl.robots_ignored row for a job. The
// event_id is derived from the job id, so audit-core's idempotency makes a
// repeated write for the same job a duplicate, not a second row.
func recordRobotsIgnored(ctx context.Context, sink RobotsAuditor, job store.Job, req robotsIgnoreRequest, requestID string) error {
	if sink == nil {
		return audit.ErrNotConfigured
	}
	details := map[string]any{
		"seed_host": req.SeedHost,
		"authority": req.Authority,
		"kind":      job.Kind,
	}
	if req.Rev != nil {
		details["rev"] = req.Rev
	}
	ctx, cancel := context.WithTimeout(ctx, robotsAuditTimeout)
	defer cancel()
	return sink.RecordAudit(ctx, audit.Event{
		EventID:    "quarry-control:" + RobotsIgnoredEvent + ":" + string(job.ID),
		OccurredAt: time.Now().UTC(),
		OrgID:      job.OrgID,
		UserID:     req.UserID,
		Event:      RobotsIgnoredEvent,
		Subject:    "quarry.job",
		ResourceID: string(job.ID),
		Outcome:    "ok",
		Details:    details,
		RequestID:  requestID,
	})
}

// createJobInput is the POST /v1/jobs body. Preset was removed in Q4-E4.1-7: the
// field is still decoded so resolveJobPolicy refuses it with a 400, not ignores it.
type createJobInput struct {
	Kind   string                     `json:"kind"`
	Policy *quarrycontracts.RunPolicy `json:"policy"`
	Preset string                     `json:"preset,omitempty"`
	Params map[string]any             `json:"params,omitempty"`
	// Origin says who asked for the job. Set by the edge from the verified
	// caller; absent means an API client.
	Origin *store.JobOrigin `json:"origin,omitempty"`
}

// resolveJobOrigin checks the origin a create request declares against the
// closed set in store.OriginTypes. An absent origin is "api"; an empty type is
// "api"; any other unknown type is refused, so a typo never lands as an origin.
func resolveJobOrigin(in *store.JobOrigin) (store.JobOrigin, error) {
	if in == nil {
		return store.JobOrigin{Type: "api"}, nil
	}
	t := strings.TrimSpace(in.Type)
	if t == "" {
		t = "api"
	}
	if !store.OriginTypes[t] {
		return store.JobOrigin{}, fmt.Errorf("origin.type must be one of user, agent, schedule, webhook, api")
	}
	if len(in.Actor) > 256 || len(in.Ref) > 256 || len(in.CreatedBy) > 256 {
		return store.JobOrigin{}, fmt.Errorf("origin fields are limited to 256 characters")
	}
	return store.JobOrigin{Type: t, Actor: in.Actor, Ref: in.Ref, CreatedBy: in.CreatedBy}, nil
}

// resolveJobPolicy honors the precedence: explicit policy > default. The Go job
// presets were removed in Q4-E4.1-7, so a non-empty `preset` is refused with a
// 400 before anything else, never ignored: a caller that still sends one must
// send `policy` instead.
func resolveJobPolicy(in createJobInput) (quarrycontracts.RunPolicy, error) {
	if in.Preset != "" {
		return quarrycontracts.RunPolicy{}, fmt.Errorf("preset was removed; send policy")
	}
	if in.Policy != nil {
		return *in.Policy, nil
	}
	return quarrycontracts.DefaultRunPolicy(), nil
}

// getJob backs GET /v1/jobs/{id}, scoped to the edge-verified `?org_id`
// query param via GetByOrg — a cross-tenant id reads as 404, same as
// getSourceHandler-style guards elsewhere in this package.
func getJob(db store.DB) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		org := orgFromQuery(r)
		if org == "" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
			return
		}
		id := quarrycontracts.ID(chi.URLParam(r, "id"))
		v, ok := db.Jobs().GetByOrg(org, id)
		if !ok {
			httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": id})
			return
		}
		httpx.WriteJSON(w, r, http.StatusOK, withoutWebhookSecret(v))
	}
}

// updateJob accepts a partial update of the mutable fields on a job
// (status + run_id). UNSCOPED BY DESIGN: it's called only by the
// orchestrator's jobs dispatcher (markRunning/markFailed in
// services/quarry-orchestrator/internal/jobs/dispatcher.go) to flip
// accepted → running/failed and stamp the Temporal run_id — a trusted
// internal service transition by job ID, not a tenant-facing read, and
// the dispatcher never sends org_id. The existing kind/params/policy/
// created_at fields are preserved so a caller can send
// `{"status": "running", "run_id": "run_…"}` without re-sending the
// original create payload.
func updateJob(db store.DB) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		id := quarrycontracts.ID(chi.URLParam(r, "id"))
		existing, ok := db.Jobs().Get(id)
		if !ok {
			httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": id})
			return
		}
		var patch struct {
			Status     *string                    `json:"status,omitempty"`
			RunID      *quarrycontracts.ID        `json:"run_id,omitempty"`
			Policy     *quarrycontracts.RunPolicy `json:"policy,omitempty"`
			WorkflowID *string                    `json:"workflow_id,omitempty"`
			DeadlineAt *string                    `json:"deadline_at,omitempty"`
			// IfStatus makes the write a compare-and-set: it applies only while the
			// stored status is still IfStatus, and otherwise answers 409.
			IfStatus *string `json:"if_status,omitempty"`
		}
		if err := json.NewDecoder(r.Body).Decode(&patch); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		if patch.Status != nil {
			// A job that ended stays ended: no patch moves it to another status.
			if store.IsTerminalJobStatus(existing.Status) && *patch.Status != existing.Status {
				httpx.WriteErr(w, r, quarrycontracts.CodeConflict, "the job has already ended",
					map[string]any{"id": id, "status": existing.Status})
				return
			}
			existing.Status = *patch.Status
			if store.IsTerminalJobStatus(existing.Status) && existing.FinishedAt == nil {
				now := time.Now().UnixMilli()
				existing.FinishedAt = &now
			}
		}
		if patch.RunID != nil {
			existing.RunID = patch.RunID
		}
		if patch.Policy != nil {
			existing.Policy = *patch.Policy
		}
		if patch.WorkflowID != nil {
			existing.WorkflowID = *patch.WorkflowID
		}
		if patch.DeadlineAt != nil {
			t, err := time.Parse(time.RFC3339Nano, *patch.DeadlineAt)
			if err != nil {
				httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "deadline_at must be RFC 3339", nil)
				return
			}
			ms := t.UnixMilli()
			existing.DeadlineAt = &ms
		}
		var err error
		if patch.IfStatus != nil {
			err = db.Jobs().UpdateIfStatus(existing, *patch.IfStatus)
		} else {
			err = db.Jobs().Update(existing)
		}
		if err != nil {
			switch {
			case errors.Is(err, store.ErrNotFound):
				httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": id})
			case errors.Is(err, store.ErrConflict):
				httpx.WriteErr(w, r, quarrycontracts.CodeConflict, "the job changed since it was read",
					map[string]any{"id": id})
			default:
				httpx.WriteErr(w, r, quarrycontracts.CodeInternal, err.Error(), nil)
			}
			return
		}
		httpx.WriteJSON(w, r, http.StatusOK, existing)
	}
}

// jobEvents backs GET /v1/jobs/{id}/events. Confirms the id belongs to the
// caller's org via GetByOrg before returning any events — quarry-edge's
// list_job_events already sends the verified org_id on every call
// (resource_routes.rs's forward_one always includes it), so this adds no
// new requirement for that caller, only closes the gap for a direct or
// future caller that omits it.
func jobEvents(db store.DB) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		org := orgFromQuery(r)
		if org == "" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
			return
		}
		id := quarrycontracts.ID(chi.URLParam(r, "id"))
		if _, ok := db.Jobs().GetByOrg(org, id); !ok {
			httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": id})
			return
		}
		after, _ := strconv.ParseUint(r.URL.Query().Get("after_seq"), 10, 64)
		limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
		if limit <= 0 {
			limit = 100
		}
		evts := db.Events().ForJob(id, after, limit)
		httpx.WriteJSON(w, r, http.StatusOK, evts)
	}
}

// jobHistory returns the merged view for a job: job record + its events.
// Contract §1.2: GET /v1/jobs/{id}/history. Scoped to the edge-verified
// `?org_id` via GetByOrg, same guard as getJob/jobEvents.
func jobHistory(db store.DB) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		id := quarrycontracts.ID(chi.URLParam(r, "id"))
		if err := id.MustKind(quarrycontracts.KindJob); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		org := orgFromQuery(r)
		if org == "" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
			return
		}
		job, ok := db.Jobs().GetByOrg(org, id)
		if !ok {
			httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": id})
			return
		}
		after, _ := strconv.ParseUint(r.URL.Query().Get("after_seq"), 10, 64)
		limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
		if limit <= 0 {
			limit = 500
		}
		evts := db.Events().ForJob(id, after, limit)
		httpx.WriteJSON(w, r, http.StatusOK, map[string]any{
			"job":    job,
			"events": evts,
		})
	}
}

// fanoutMaxSubscriptions bounds the subscriptions one event is matched against. It is above
// the per-org subscription cap, so it only bites on rows written around the API.
const fanoutMaxSubscriptions = 100

// fanoutWebhooks creates pending WebhookDelivery rows for every active subscription OF
// ORGID whose Events filter matches the event. The dispatcher worker pool will claim and
// POST them with HMAC signing and retry/backoff.
//
// Tenancy (INNH-5): orgID is the org that owns the event, resolved by the caller from
// something it verified (the change webhook's signed org, or the run's job). An empty orgID
// delivers to nobody: an event whose owner is unknown must not reach any tenant. A legacy
// subscription with no org is never listed by ListActiveByOrg, and an empty Events list
// matches nothing; "*" matches every event type of that org only.
func fanoutWebhooks(db store.DB, ev quarrycontracts.Event, orgID string) {
	if orgID == "" {
		return
	}
	payloadBytes, err := json.Marshal(ev)
	if err != nil {
		return
	}
	now := time.Now().Unix()
	evType := string(ev.Type)
	dedupe := changeDedupeKey(ev, orgID)
	for _, wh := range db.Webhooks().ListActiveByOrg(orgID, fanoutMaxSubscriptions) {
		if wh.OrgID != orgID || !subscriptionMatches(wh.Events, evType) {
			continue
		}
		// A duplicate (ErrConflict) means this subscription already holds a delivery for
		// the same logical event, reached by another path: drop it silently.
		_ = db.WebhookDeliveries().Create(store.WebhookDelivery{
			ID:            quarrycontracts.NewID(quarrycontracts.KindWebhookDelivery),
			WebhookID:     wh.ID,
			OrgID:         orgID,
			EventID:       ev.EventID,
			EventType:     evType,
			DedupeKey:     dedupe,
			Payload:       string(payloadBytes),
			Status:        "pending",
			Attempt:       0,
			NextAttemptAt: now,
			CreatedAt:     now,
		})
	}
}

// fanoutJobWebhook creates the pending delivery of one run event to its job's own webhook
// (params.webhook, W3-E3.5-3c), as fanoutWebhooks does for a subscription. The event's run must
// belong to orgID, the org that owns the event: the job is found by run id and then compared with
// that org, so a run of another tenant is never delivered. The job's stored hook must want the
// event type. Nothing is created while QUARRY_CONTROL__JOB_WEBHOOK is off, and a ZDR job has no
// delivery rows, so its payload is never persisted.
func fanoutJobWebhook(db store.DB, ev quarrycontracts.Event, orgID string) {
	if orgID == "" || ev.RunID == nil || !JobWebhookEvents[string(ev.Type)] || !JobWebhooksEnabled(jobWebhookGetenv) {
		return
	}
	owner, found := db.Jobs().GetByRunID(*ev.RunID)
	if !found || owner.OrgID != orgID {
		return
	}
	hook, ok := JobWebhookTarget(owner)
	if !ok || !hook.Wants(string(ev.Type)) {
		return
	}
	payloadBytes, err := json.Marshal(ev)
	if err != nil {
		return
	}
	now := time.Now().Unix()
	jobID := string(owner.ID)
	// A duplicate (ErrConflict) is the same event reached by another path: dropped silently.
	_ = db.WebhookDeliveries().Create(store.WebhookDelivery{
		ID:            quarrycontracts.NewID(quarrycontracts.KindWebhookDelivery),
		WebhookID:     JobWebhookDeliveryWebhookID(jobID),
		OrgID:         orgID,
		EventID:       ev.EventID,
		EventType:     string(ev.Type),
		DedupeKey:     JobWebhookDeliveryPrefix + jobID + ":" + string(ev.EventID),
		Payload:       string(payloadBytes),
		Status:        "pending",
		Attempt:       0,
		NextAttemptAt: now,
		CreatedAt:     now,
	})
}

// subscriptionMatches reports whether a subscription's event filter accepts evType.
func subscriptionMatches(events []string, evType string) bool {
	for _, et := range events {
		if et == "*" || et == evType {
			return true
		}
	}
	return false
}

// changeDedupeKey names one detected change: org, url and the new baseline. The same change
// reaches control as the edge's signed webhook and as a run event (and the scheduled-check
// workflow emits a third), each with its own event id; a subscription must get it once.
// Empty when the event is not a change or the payload lacks either part, which turns the
// dedupe off for that event rather than guessing.
func changeDedupeKey(ev quarrycontracts.Event, orgID string) string {
	if ev.Type != quarrycontracts.EvtChangeDetected {
		return ""
	}
	url := payloadString(ev.Payload, "source_url")
	if url == "" {
		url = payloadString(ev.Payload, "url")
	}
	baseline := ""
	if nb, ok := ev.Payload["new_baseline"].(map[string]any); ok {
		baseline = payloadString(nb, "baseline_id")
	}
	if baseline == "" {
		baseline = payloadString(ev.Payload, "baseline_id")
	}
	if url == "" || baseline == "" {
		return ""
	}
	return "change_detected:" + orgID + ":" + url + ":" + baseline
}
