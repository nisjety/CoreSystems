package resources

// Per-job results (INNH-12), the dataset equivalent of a crawl or batch run: one row for
// each page the job fetched, read from the job's page_fetched events. Nothing is written.
// A row carries the page URL, closed fetch metadata and the ids of the stored artifacts;
// the content itself stays behind the org-bound artifact read. Every read is scoped to the
// edge-verified org: a foreign id is a 404, as on the errors route.

import (
	"encoding/json"
	"net/http"
	"sort"
	"strconv"
	"time"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

const (
	// resultsDefaultLimit and resultsMaxLimit bound one page of results.
	resultsDefaultLimit = 100
	resultsMaxLimit     = 500
	// resultsLatestScanCap is the most page_fetched events a latest=true read inspects. A
	// job with more says so in scan_capped instead of silently answering from a prefix.
	resultsLatestScanCap = 20000
)

// resultEventTypes are the events a result row is read from. Failed and blocked pages are
// not results: they stay on the errors route.
var resultEventTypes = []quarrycontracts.EventType{quarrycontracts.EvtPageFetched}

// jobResultView is one row of GET /v1/jobs/{id}/results. Fields after fetched_at are left
// out when the page_fetched event predates them or nothing was stored (ZDR never stores).
type jobResultView struct {
	Seq                uint64 `json:"seq"`
	URL                string `json:"url"`
	Status             int    `json:"status"`
	ContentType        string `json:"content_type,omitempty"`
	Title              string `json:"title,omitempty"`
	FetchedAt          string `json:"fetched_at"`
	WordCount          uint64 `json:"word_count,omitempty"`
	Driver             string `json:"driver,omitempty"`
	MarkdownArtifactID string `json:"markdown_artifact_id,omitempty"`
	SourceArtifactID   string `json:"source_artifact_id,omitempty"`
	Bytes              uint64 `json:"bytes,omitempty"`
}

// jobResultsView is GET /v1/jobs/{id}/results. next_seq is set when more rows remain: pass
// it back as after_seq to read them.
type jobResultsView struct {
	Items      []jobResultView `json:"items"`
	NextSeq    *uint64         `json:"next_seq,omitempty"`
	ScanCapped bool            `json:"scan_capped,omitempty"`
}

// payloadString reads an optional string from an event payload.
func payloadString(p map[string]any, key string) string {
	s, _ := p[key].(string)
	return s
}

// payloadUint reads an optional non-negative number from an event payload. A payload read
// back from JSON holds float64; one built in process may hold any integer type.
func payloadUint(p map[string]any, key string) uint64 {
	switch v := p[key].(type) {
	case float64:
		if v > 0 {
			return uint64(v)
		}
	case int:
		if v > 0 {
			return uint64(v)
		}
	case int64:
		if v > 0 {
			return uint64(v)
		}
	case uint16:
		return uint64(v)
	case uint64:
		return v
	case json.Number:
		if n, err := v.Int64(); err == nil && n > 0 {
			return uint64(n)
		}
	}
	return 0
}

// resultFromEvent turns one page_fetched event into a row. An event written before the
// result fields existed yields a row with url, status and title only.
func resultFromEvent(ev quarrycontracts.Event) jobResultView {
	return jobResultView{
		Seq:                ev.Seq,
		URL:                payloadString(ev.Payload, "url"),
		Status:             int(payloadUint(ev.Payload, "status")),
		ContentType:        payloadString(ev.Payload, "content_type"),
		Title:              payloadString(ev.Payload, "title"),
		FetchedAt:          ev.Timestamp.UTC().Format(time.RFC3339Nano),
		WordCount:          payloadUint(ev.Payload, "word_count"),
		Driver:             payloadString(ev.Payload, "driver"),
		MarkdownArtifactID: payloadString(ev.Payload, "markdown_artifact_id"),
		SourceArtifactID:   payloadString(ev.Payload, "source_artifact_id"),
		Bytes:              payloadUint(ev.Payload, "bytes"),
	}
}

// buildResults turns a page of page_fetched events into the results view. A URL that was
// fetched twice (a retry, an escalation to a browser) appears twice. The cursor is set when
// the page was full, like the errors view.
func buildResults(events []quarrycontracts.Event, limit int) jobResultsView {
	view := jobResultsView{Items: make([]jobResultView, 0, len(events))}
	for _, ev := range events {
		view.Items = append(view.Items, resultFromEvent(ev))
	}
	if limit > 0 && len(events) >= limit && len(events) > 0 {
		next := events[len(events)-1].Seq
		view.NextSeq = &next
	}
	return view
}

// latestResults is the dataset view: one row per URL, the latest fetch of it, in the seq
// order of those rows. It reads the job's page_fetched events from the start (a row's
// dedupe depends on later rows), at most resultsLatestScanCap of them, then pages the
// deduplicated rows by after_seq. next_seq is set when more rows remain.
func latestResults(db store.DB, id quarrycontracts.ID, after uint64, limit int) jobResultsView {
	view := jobResultsView{Items: []jobResultView{}}
	byURL := map[string]jobResultView{}
	var cursor uint64
	scanned := 0
	for {
		batch := db.Events().ForJobTypes(id, resultEventTypes, cursor, resultsMaxLimit)
		for _, ev := range batch {
			row := resultFromEvent(ev)
			byURL[row.URL] = row
			cursor = ev.Seq
		}
		scanned += len(batch)
		if len(batch) < resultsMaxLimit {
			break
		}
		if scanned >= resultsLatestScanCap {
			// A full batch followed the ceiling: events remain unread.
			view.ScanCapped = true
			break
		}
	}
	rows := make([]jobResultView, 0, len(byURL))
	for _, row := range byURL {
		if row.Seq > after {
			rows = append(rows, row)
		}
	}
	sort.Slice(rows, func(i, j int) bool { return rows[i].Seq < rows[j].Seq })
	if len(rows) > limit {
		next := rows[limit-1].Seq
		view.NextSeq = &next
		rows = rows[:limit]
	}
	view.Items = append(view.Items, rows...)
	return view
}

// jobResults backs GET /v1/jobs/{id}/results, paged by event seq (after_seq, limit; default
// 100, at most 500). latest=true keeps the latest fetch of each URL.
func jobResults(db store.DB) http.HandlerFunc {
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
		q := r.URL.Query()
		after, _ := strconv.ParseUint(q.Get("after_seq"), 10, 64)
		limit := jobPageLimit(q.Get("limit"), resultsDefaultLimit, resultsMaxLimit)
		if latest, _ := strconv.ParseBool(q.Get("latest")); latest {
			httpx.WriteJSON(w, r, http.StatusOK, latestResults(db, id, after, limit))
			return
		}
		events := db.Events().ForJobTypes(id, resultEventTypes, after, limit)
		httpx.WriteJSON(w, r, http.StatusOK, buildResults(events, limit))
	}
}
