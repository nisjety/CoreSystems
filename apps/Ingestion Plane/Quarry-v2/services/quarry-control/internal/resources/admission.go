package resources

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/rs/zerolog"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// Admission is the job-admission policy. Before it, the dispatcher took the
// newest 50 jobs across every organization and started all of them, so one
// organization's backlog could fill the window and starve everyone else, and
// there was no bound on how much one tenant could queue.
//
// With admission on, an organization never has more than its cap of running or
// leased jobs; the rest wait in `accepted`, oldest first, up to a bound and a
// time limit, and the dispatcher takes work by claiming it (round-robin across
// organizations) instead of reading a window.
type Admission struct {
	// Enabled turns the whole policy on. Off (the default) leaves control's
	// behaviour exactly as it was: no backlog bound, no claim endpoint.
	Enabled bool
	// CapDefault is the per-organization concurrent-job cap used when a job
	// carries none (params.admission.cap).
	CapDefault int
	// BacklogDefault bounds an organization's waiting jobs when a job carries no
	// params.admission.backlog_cap.
	BacklogDefault int
	// BacklogTTL is how long a job may wait before it is failed with
	// failure_reason "backlog_expired".
	BacklogTTL time.Duration
	// ClaimLease is how long a claimed job is reserved for the dispatcher to
	// start it; past that it can be claimed again.
	ClaimLease time.Duration
}

const (
	maxClaimBatch  = 200
	retryAfterSecs = 30
)

// AdmissionFromEnv reads the policy. Unset values take the defaults; a value
// that is not a positive number or duration is an error naming the variable.
func AdmissionFromEnv(get func(string) string) (Admission, error) {
	a := Admission{CapDefault: 3, BacklogDefault: 200, BacklogTTL: 24 * time.Hour, ClaimLease: 60 * time.Second}
	switch strings.ToLower(strings.TrimSpace(get("QUARRY_JOB_ADMISSION"))) {
	case "", "off", "0", "false":
	case "on", "1", "true":
		a.Enabled = true
	default:
		return Admission{}, fmt.Errorf("QUARRY_JOB_ADMISSION must be on or off")
	}
	positiveInt := func(name string, into *int) error {
		raw := strings.TrimSpace(get(name))
		if raw == "" {
			return nil
		}
		v, err := strconv.Atoi(raw)
		if err != nil || v < 1 {
			return fmt.Errorf("%s must be a positive integer", name)
		}
		*into = v
		return nil
	}
	positiveDuration := func(name string, into *time.Duration) error {
		raw := strings.TrimSpace(get(name))
		if raw == "" {
			return nil
		}
		v, err := time.ParseDuration(raw)
		if err != nil || v <= 0 {
			return fmt.Errorf("%s must be a positive duration such as 60s or 24h", name)
		}
		*into = v
		return nil
	}
	if err := positiveInt("QUARRY_JOB_CAP_DEFAULT", &a.CapDefault); err != nil {
		return Admission{}, err
	}
	if err := positiveInt("QUARRY_JOB_BACKLOG_DEFAULT", &a.BacklogDefault); err != nil {
		return Admission{}, err
	}
	if err := positiveDuration("QUARRY_JOB_BACKLOG_TTL", &a.BacklogTTL); err != nil {
		return Admission{}, err
	}
	if err := positiveDuration("QUARRY_JOB_CLAIM_LEASE", &a.ClaimLease); err != nil {
		return Admission{}, err
	}
	return a, nil
}

// backlogCap is the job's own params.admission.backlog_cap when it is a
// positive number, otherwise the default.
func (a Admission) backlogCap(params map[string]any) int {
	admission, ok := params["admission"].(map[string]any)
	if !ok {
		return a.BacklogDefault
	}
	if v, ok := admission["backlog_cap"].(float64); ok && v >= 1 && v <= 1_000_000 {
		return int(v)
	}
	return a.BacklogDefault
}

// backlogFull is the 429 a submission gets when the organization's waiting
// jobs already reach its bound. It is checked before anything is stored, so a
// refused submission leaves no row.
func (a Admission) backlogFull(db store.DB, org, kind string, params map[string]any, now time.Time) (map[string]any, bool) {
	if !a.Enabled || !store.AdmissionKind(kind) {
		return nil, false
	}
	limit := a.backlogCap(params)
	queued := db.Jobs().CountAccepted(org, now)
	if queued < limit {
		return nil, false
	}
	return map[string]any{
		"reason":              "org_backlog_full",
		"limit":               limit,
		"queued":              queued,
		"retry_after_seconds": retryAfterSecs,
	}, true
}

// claimJobs backs POST /v1/internal/jobs/claim {limit}: the dispatcher's
// replacement for reading the newest jobs. Not found while admission is off, so
// a dispatcher that already speaks the claim protocol falls back to its old
// list during a rolling deploy.
func claimJobs(db store.DB, a Admission) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if !a.Enabled {
			httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "job admission is not enabled", nil)
			return
		}
		var in struct {
			Limit int `json:"limit"`
		}
		if err := json.NewDecoder(r.Body).Decode(&in); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, err.Error(), nil)
			return
		}
		limit := in.Limit
		if limit <= 0 || limit > maxClaimBatch {
			limit = maxClaimBatch
		}
		jobs, err := db.Jobs().ClaimAccepted(limit, time.Now(), a.ClaimLease, a.CapDefault)
		if err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeInternal, err.Error(), nil)
			return
		}
		if jobs == nil {
			jobs = []store.Job{}
		}
		httpx.WriteJSON(w, r, http.StatusOK, jobs)
	}
}

// ExpireBacklog fails the waiting jobs that have been waiting longer than the
// TTL and closes their event streams with a terminal event, so a client that
// is polling or streaming a job that will never start learns that it ended.
// It returns how many it failed.
func ExpireBacklog(db store.DB, a Admission, now time.Time, logger zerolog.Logger) int {
	failed, err := db.Jobs().FailExpired(now.Add(-a.BacklogTTL), now)
	if err != nil {
		logger.Warn().Err(err).Msg("job backlog expiry failed")
		return 0
	}
	for _, job := range failed {
		id := job.ID
		event := quarrycontracts.Event{
			EventID:   quarrycontracts.NewID(quarrycontracts.KindEvent),
			JobID:     &id,
			Type:      quarrycontracts.EvtRunFailed,
			Timestamp: now.UTC(),
			Seq:       1,
			Payload:   map[string]any{"reason": "backlog_expired", "kind": job.Kind},
		}
		if err := db.Events().Append(event); err != nil {
			logger.Warn().Err(err).Str("job_id", string(job.ID)).Msg("backlog expiry: terminal event not recorded")
		}
	}
	if len(failed) > 0 {
		logger.Info().Int("failed", len(failed)).Msg("job backlog expired")
	}
	return len(failed)
}

// RunBacklogJanitor expires old waiting jobs every interval until ctx ends. A
// no-op while admission is off.
func RunBacklogJanitor(ctx context.Context, db store.DB, a Admission, interval time.Duration, logger zerolog.Logger) {
	if !a.Enabled {
		return
	}
	if interval <= 0 {
		interval = time.Minute
	}
	ticker := time.NewTicker(interval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			ExpireBacklog(db, a, time.Now(), logger)
		}
	}
}
