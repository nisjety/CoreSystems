package resources

// Per-job credits (INNH-9). A derived, read-time estimate: the job's
// page_fetched count times the shared per-page price. Nothing is stored, so
// the figure is retroactive for old jobs, needs no migration and cannot go
// stale when a late page_fetched lands after the terminal event. It rides on
// the job summary and the job list only while QUARRY_CONTROL__JOB_CREDITS is on.

import (
	"os"
	"strings"

	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// jobCreditsEnv gates the credits object on the job reads. Default off: the
// per-page price is a placeholder, so the number stays hidden until the owner
// approves its semantics.
const jobCreditsEnv = "QUARRY_CONTROL__JOB_CREDITS"

// jobCreditsGetenv is os.Getenv; a test swaps it to flip the flag without the
// process environment.
var jobCreditsGetenv = os.Getenv

// jobCreditsEnabled reads the flag per request, so no restart is needed.
func jobCreditsEnabled() bool {
	switch strings.ToLower(strings.TrimSpace(jobCreditsGetenv(jobCreditsEnv))) {
	case "1", "true", "on", "yes":
		return true
	}
	return false
}

// jobCredits is the credits object of a job. used = completed pages x
// price_per_page. estimated is always true: this is a Quarry-side estimate,
// not what billing-core invoices. final is false while the job can still fetch
// pages and true once its status is terminal.
type jobCredits struct {
	Used         float64 `json:"used"`
	Unit         string  `json:"unit"`
	Basis        string  `json:"basis"`
	PricePerPage float64 `json:"price_per_page"`
	Estimated    bool    `json:"estimated"`
	Final        bool    `json:"final"`
}

// creditKinds are the job kinds priced by pages fetched. Search, research and
// agent runs are not page-priced (agent cost is browser seconds and model
// tokens, which control's event log does not carry), so they get no credits
// object rather than a misleading zero.
var creditKinds = map[string]bool{
	"crawl": true, "batch": true, "scrape": true, "extract": true,
}

// creditsFor derives the credits of a job from its kind, its fetched-page
// count and its status. It returns nil for a kind that is not page-priced.
// Pure: no I/O.
func creditsFor(kind, status string, fetchedPages int64) *jobCredits {
	if !creditKinds[kind] {
		return nil
	}
	if fetchedPages < 0 {
		fetchedPages = 0
	}
	return &jobCredits{
		Used:         float64(fetchedPages) * store.CreditsPerPage,
		Unit:         "credit",
		Basis:        "pages_fetched",
		PricePerPage: store.CreditsPerPage,
		Estimated:    true,
		Final:        store.IsTerminalJobStatus(status),
	}
}
