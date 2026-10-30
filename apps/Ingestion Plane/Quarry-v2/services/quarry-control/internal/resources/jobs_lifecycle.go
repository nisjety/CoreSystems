package resources

// Job lifecycle read surface (WS15.L1): the summary, the errors and the
// ongoing list of a crawl, batch or scrape job. Everything here is derived
// from the job record and its event log at read time; nothing is written.
// Every read is scoped to the edge-verified org: a foreign id is a 404.

import (
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// jobCounts are the page counters of a job, from its event log.
type jobCounts struct {
	Completed int64 `json:"completed"`
	Failed    int64 `json:"failed"`
	Blocked   int64 `json:"blocked"`
	Retried   int64 `json:"retried"`
	Extracted int64 `json:"extracted"`
}

// jobWarning is a non-fatal note about a job, with a closed code.
type jobWarning struct {
	Code    string `json:"code"`
	Message string `json:"message"`
}

// jobSummaryView is GET /v1/jobs/{id}/summary. The edge reshapes it into the
// public status document; this is the control-side truth.
type jobSummaryView struct {
	Job        store.Job    `json:"job"`
	Phase      string       `json:"phase"`
	Counts     jobCounts    `json:"counts"`
	Total      int          `json:"total"`
	TotalKind  string       `json:"total_kind,omitempty"`
	LastSeq    uint64       `json:"last_seq"`
	FirstTS    *string      `json:"first_ts,omitempty"`
	LastTS     *string      `json:"last_ts,omitempty"`
	DeadlineAt *string      `json:"deadline_at,omitempty"`
	Warnings   []jobWarning `json:"warnings"`
	// Credits is the derived per-job credit estimate, present only for
	// page-priced kinds and only while QUARRY_CONTROL__JOB_CREDITS is on.
	Credits *jobCredits `json:"credits,omitempty"`
}

// jobErrorView is one entry of GET /v1/jobs/{id}/errors. The raw error text a
// worker recorded is deliberately not carried: the category and code are the
// closed vocabulary the owner may rely on.
type jobErrorView struct {
	ID        string `json:"id"`
	Seq       uint64 `json:"seq"`
	TS        string `json:"ts"`
	URL       string `json:"url,omitempty"`
	Code      string `json:"code,omitempty"`
	Category  string `json:"category,omitempty"`
	Retryable bool   `json:"retryable"`
	Status    int    `json:"status,omitempty"`
}

// jobBlockedView is one entry of the blocked list of GET /v1/jobs/{id}/errors.
type jobBlockedView struct {
	URL    string `json:"url,omitempty"`
	TS     string `json:"ts"`
	Reason string `json:"reason,omitempty"`
}

// jobErrorsView is GET /v1/jobs/{id}/errors. next_seq is set when the page was
// full: pass it back as after_seq to read the rest.
type jobErrorsView struct {
	Errors  []jobErrorView   `json:"errors"`
	Blocked []jobBlockedView `json:"blocked"`
	NextSeq *uint64          `json:"next_seq,omitempty"`
}

// jobPageLimit clamps a requested page size to [1, max], defaulting to def.
func jobPageLimit(raw string, def, max int) int {
	n, err := strconv.Atoi(raw)
	if err != nil || n <= 0 {
		return def
	}
	if n > max {
		return max
	}
	return n
}

// jobStatusVocabulary is the closed set of job statuses a list may filter on.
var jobStatusVocabulary = map[string]bool{
	"accepted": true, "running": true, "paused": true, "completed": true,
	"failed": true, "cancelled": true, "timed_out": true, "skipped": true,
}

// jobStatusFilter parses ?status=a,b. An empty value is no filter; an unknown
// status makes ok false, so the caller refuses the request.
func jobStatusFilter(raw string) (statuses []string, ok bool) {
	if strings.TrimSpace(raw) == "" {
		return nil, true
	}
	for _, part := range strings.Split(raw, ",") {
		s := strings.TrimSpace(part)
		if s == "" {
			continue
		}
		if !jobStatusVocabulary[s] {
			return nil, false
		}
		statuses = append(statuses, s)
	}
	return statuses, true
}

// optRFC3339Millis renders an optional unix-millis timestamp, nil when unset.
func optRFC3339Millis(ms *int64) *string {
	if ms == nil {
		return nil
	}
	s := time.UnixMilli(*ms).UTC().Format(time.RFC3339Nano)
	return &s
}

// paramNumber reads a numeric job parameter; JSON numbers decode as float64.
func paramNumber(params map[string]any, key string) (int, bool) {
	switch v := params[key].(type) {
	case float64:
		return int(v), true
	case int:
		return v, true
	case int64:
		return int(v), true
	}
	return 0, false
}

// jobTotal is the job's page total and how it is known: the exact URL count of
// a batch, or the effective page ceiling of a crawl ("budget": a ceiling, not a
// prediction). Other kinds report no total.
func jobTotal(job store.Job) (int, string) {
	switch job.Kind {
	case "batch":
		if urls, ok := job.Params["urls"].([]any); ok {
			return len(urls), "exact"
		}
	case "crawl":
		if n, ok := paramNumber(job.Params, "max_pages"); ok {
			return n, "budget"
		}
	}
	return 0, ""
}

// jobPhase maps a status onto the phase a person reads: a job that is still
// waiting is "queued"; the rest carry their status.
func jobPhase(status string) string {
	if status == "accepted" {
		return "queued"
	}
	return status
}

// countsFrom reads the page counters from a per-type event summary.
func countsFrom(sum map[quarrycontracts.EventType]store.EventTypeSummary) jobCounts {
	return jobCounts{
		Completed: sum[quarrycontracts.EventType("page_fetched")].Count,
		Failed:    sum[quarrycontracts.EventType("page_failed")].Count,
		Blocked:   sum[quarrycontracts.EventType("page_blocked")].Count,
		Retried:   sum[quarrycontracts.EventType("page_retried")].Count,
		Extracted: sum[quarrycontracts.EventType("page_extracted")].Count,
	}
}

// buildSummary assembles the summary of one job from its record and its
// per-type event summary. Pure: no I/O, so the rules are unit-testable.
func buildSummary(job store.Job, sum map[quarrycontracts.EventType]store.EventTypeSummary) jobSummaryView {
	view := jobSummaryView{
		Job:        job,
		Phase:      jobPhase(job.Status),
		Counts:     countsFrom(sum),
		Warnings:   []jobWarning{},
		DeadlineAt: optRFC3339Millis(job.DeadlineAt),
	}
	view.Total, view.TotalKind = jobTotal(job)
	view.Credits = creditsFor(job.Kind, job.Status, view.Counts.Completed)

	var first, last time.Time
	for _, s := range sum {
		if s.MaxSeq > view.LastSeq {
			view.LastSeq = s.MaxSeq
		}
		if first.IsZero() || (!s.First.IsZero() && s.First.Before(first)) {
			first = s.First
		}
		if s.Last.After(last) {
			last = s.Last
		}
	}
	if !first.IsZero() {
		f := first.UTC().Format(time.RFC3339Nano)
		view.FirstTS = &f
	}
	if !last.IsZero() {
		l := last.UTC().Format(time.RFC3339Nano)
		view.LastTS = &l
	}
	// A crawl whose requested page limit was lowered to the ceiling says so; a
	// clamp is never silent.
	if requested, ok := paramNumber(job.Params, "max_pages_requested"); ok && view.TotalKind == "budget" && requested > view.Total {
		view.Warnings = append(view.Warnings, jobWarning{
			Code:    "max_pages_clamped",
			Message: "the requested page limit was lowered to the service ceiling",
		})
	}
	return view
}

// errorEventTypes are the events an error or blocked entry is read from.
var errorEventTypes = []quarrycontracts.EventType{
	quarrycontracts.EvtPageFailed,
	quarrycontracts.EvtPageRetried,
	quarrycontracts.EvtPageBlocked,
}

// buildErrors turns the job's error-bearing events into the errors view. A
// failed or retried page is an error only when it is final (the orchestrator
// marks the last attempt with payload.final); a blocked page is always listed
// under blocked. lastSeq is the seq of the last event read, used as the cursor.
func buildErrors(events []quarrycontracts.Event, limit int) jobErrorsView {
	view := jobErrorsView{Errors: []jobErrorView{}, Blocked: []jobBlockedView{}}
	for _, ev := range events {
		url, _ := ev.Payload["url"].(string)
		ts := ev.Timestamp.UTC().Format(time.RFC3339Nano)
		switch ev.Type {
		case quarrycontracts.EvtPageBlocked:
			reason, _ := ev.Payload["category"].(string)
			view.Blocked = append(view.Blocked, jobBlockedView{URL: url, TS: ts, Reason: reason})
		default:
			if final, _ := ev.Payload["final"].(bool); !final {
				continue
			}
			category, _ := ev.Payload["category"].(string)
			code, _ := ev.Payload["code"].(string)
			retryable, _ := ev.Payload["retryable"].(bool)
			status, _ := ev.Payload["status"].(float64)
			view.Errors = append(view.Errors, jobErrorView{
				ID:        string(ev.EventID),
				Seq:       ev.Seq,
				TS:        ts,
				URL:       url,
				Code:      code,
				Category:  category,
				Retryable: retryable,
				Status:    int(status),
			})
		}
	}
	if limit > 0 && len(events) >= limit && len(events) > 0 {
		next := events[len(events)-1].Seq
		view.NextSeq = &next
	}
	return view
}

// jobSummary backs GET /v1/jobs/{id}/summary.
func jobSummary(db store.DB) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		org := orgFromQuery(r)
		if org == "" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
			return
		}
		id := quarrycontracts.ID(chi.URLParam(r, "id"))
		job, ok := db.Jobs().GetByOrg(org, id)
		if !ok {
			httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": id})
			return
		}
		view := buildSummary(job, db.Events().SummaryForJob(id))
		if !jobCreditsEnabled() {
			view.Credits = nil
		}
		httpx.WriteJSON(w, r, http.StatusOK, view)
	}
}

// jobErrors backs GET /v1/jobs/{id}/errors, paged by event seq (after_seq,
// limit; default 100, at most 500).
func jobErrors(db store.DB) http.HandlerFunc {
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
		limit := jobPageLimit(r.URL.Query().Get("limit"), 100, 500)
		events := db.Events().ForJobTypes(id, errorEventTypes, after, limit)
		httpx.WriteJSON(w, r, http.StatusOK, buildErrors(events, limit))
	}
}

// jobsOngoing backs GET /v1/jobs/ongoing: the org's jobs that are still in
// flight, oldest first, with the same wire shape as the job list (stats
// included). Org-scoped; the static path takes precedence over /{id}.
func jobsOngoing(db store.DB) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		org := orgFromQuery(r)
		if org == "" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
			return
		}
		limit := jobPageLimit(r.URL.Query().Get("limit"), 50, 200)
		jobs := db.Jobs().ListActiveByOrg(org, limit)
		httpx.WriteJSON(w, r, http.StatusOK, jobWires(db, jobs))
	}
}
