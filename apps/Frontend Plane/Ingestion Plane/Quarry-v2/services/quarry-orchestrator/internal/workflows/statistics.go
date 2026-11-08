package workflows

import (
	"errors"
	"time"

	"go.temporal.io/sdk/temporal"

	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/activities"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/errs"
)

// jobStatistics is what a crawl or batch run reports about itself when it
// finishes: how many pages, how they ended, which drivers served them and how
// long it took. It lives in workflow state, so it is rebuilt identically on
// replay and adds no activity and no timer.
//
// Every key is from a closed vocabulary (a status class, an allowlisted driver
// name, an error category), never a URL, a title or an error message: the
// object is stored with the job and shown to the person who started it, and a
// crawl may visit pages they would not want echoed into a metrics blob.
type jobStatistics struct {
	startedAt     time.Time
	statusClasses map[string]uint32
	drivers       map[string]uint32
	errorClasses  map[string]uint32
}

// knownDrivers are the serving drivers the runtime reports (quarry-core
// DriverKind). Anything else is counted as "other".
var knownDrivers = map[string]struct{}{"static": {}, "browser": {}, "tls": {}}

// knownErrorCategories are errs.Category.String() values. The classification
// is done by the activity layer; a value outside this list means a category
// this binary does not know and is counted as "unknown".
var knownErrorCategories = map[string]struct{}{
	"network": {}, "timeout": {}, "server_5xx": {}, "client_4xx": {},
	"validation": {}, "blocklist": {}, "auth": {}, "unknown": {},
}

func newJobStatistics(now time.Time) *jobStatistics {
	return &jobStatistics{
		startedAt:     now,
		statusClasses: map[string]uint32{},
		drivers:       map[string]uint32{},
		errorClasses:  map[string]uint32{},
	}
}

// statusClass is "2xx".."5xx", or "other" for anything outside 200-599.
func statusClass(status uint16) string {
	switch {
	case status >= 200 && status < 300:
		return "2xx"
	case status >= 300 && status < 400:
		return "3xx"
	case status >= 400 && status < 500:
		return "4xx"
	case status >= 500 && status < 600:
		return "5xx"
	default:
		return "other"
	}
}

// recordPage counts a fetched page.
func (s *jobStatistics) recordPage(res activities.RunPageResult) {
	s.statusClasses[statusClass(res.Status)]++
	if res.Driver != "" {
		name := res.Driver
		if _, ok := knownDrivers[name]; !ok {
			name = "other"
		}
		s.drivers[name]++
	}
}

// recordFailure counts a page that could not be fetched, by error category.
func (s *jobStatistics) recordFailure(err error) {
	category := errs.CategoryUnknown.String()
	var appErr *temporal.ApplicationError
	switch classified, ok := errs.Classify(err); {
	case ok:
		category = classified.Category.String()
	case errors.As(err, &appErr):
		// An activity error reaches the workflow as a Temporal application
		// error whose Type is the category (errs.Error.Temporal).
		category = appErr.Type()
	}
	if _, ok := knownErrorCategories[category]; !ok {
		category = errs.CategoryUnknown.String()
	}
	s.errorClasses[category]++
}

// payload is the object attached to a terminal event as "statistics". Empty
// maps are omitted. Durations are whole milliseconds from the workflow clock.
func (s *jobStatistics) payload(visited, failed uint32, now time.Time) map[string]any {
	out := map[string]any{
		"pages_visited": visited,
		"pages_failed":  failed,
		"duration_ms":   now.Sub(s.startedAt).Milliseconds(),
	}
	if len(s.statusClasses) > 0 {
		out["status_classes"] = copyCounts(s.statusClasses)
	}
	if len(s.drivers) > 0 {
		out["drivers"] = copyCounts(s.drivers)
	}
	if len(s.errorClasses) > 0 {
		out["error_classes"] = copyCounts(s.errorClasses)
	}
	return out
}

func copyCounts(in map[string]uint32) map[string]uint32 {
	out := make(map[string]uint32, len(in))
	for key, value := range in {
		out[key] = value
	}
	return out
}
