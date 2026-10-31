package config

import (
	"fmt"
	"strings"
	"time"
)

const (
	defaultTenderRetention = 90 * 24 * time.Hour
	defaultTenderMaxMB     = 100
	// maxTenderMB bounds the archive an upload may be. Reading one holds the archive and one expanded
	// file in memory, for each upload that runs at once, so the bound is what the service's memory
	// limit has to cover several times over.
	maxTenderMB = 512
	// maxTenderRetention is the longest an upload is kept. The database refuses more (migration 0007),
	// and the service checks the same bound when it starts (docstore.MaxRetention).
	maxTenderRetention = 2 * 365 * 24 * time.Hour
	// minTenderRetention is the shortest, which is docstore.MinRetention: the database keeps the retention as an
	// interval to the microsecond, and a shorter one is nothing, so that every upload would expire as it was made and
	// be refused.
	minTenderRetention = time.Second
)

// TenderDocs holds the settings for the tender documents a company uploads for a notice
// (internal/tenderdocs). It is opt-in: the feature keeps a company's own documents, so an operator
// turns it on, and until then the routes are not there.
type TenderDocs struct {
	Enabled bool
	// Retention is how long an upload is kept before it expires and is purged.
	Retention time.Duration
	// MaxArchiveBytes is the largest archive an upload may be.
	MaxArchiveBytes int64
}

// loadTenderDocs reads ANBUD_TENDER_DOCS, ANBUD_TENDER_DOCS_RETENTION and ANBUD_TENDER_DOCS_MAX_MB. All
// three are checked whether or not the feature is on: a value that is wrong the day somebody turns it
// on is wrong now. A rejected value is never echoed, only its variable's name.
func loadTenderDocs(get func(string) string) (TenderDocs, []string) {
	var problems []string
	t := TenderDocs{Retention: defaultTenderRetention, MaxArchiveBytes: defaultTenderMaxMB << 20}

	switch strings.ToLower(trimmed(get, "ANBUD_TENDER_DOCS")) {
	case "", "false", "0":
	case "true", "1":
		t.Enabled = true
	default:
		problems = append(problems, "ANBUD_TENDER_DOCS must be true or false")
	}

	if raw := trimmed(get, "ANBUD_TENDER_DOCS_RETENTION"); raw != "" {
		d, err := time.ParseDuration(raw)
		if err != nil || d < minTenderRetention || d > maxTenderRetention {
			problems = append(problems, fmt.Sprintf("ANBUD_TENDER_DOCS_RETENTION must be a duration such as 2160h, at least %s and at most %s", minTenderRetention, maxTenderRetention))
		} else {
			t.Retention = d
		}
	}

	mb, problem := boundedInt(get, "ANBUD_TENDER_DOCS_MAX_MB", defaultTenderMaxMB, maxTenderMB)
	if problem != "" {
		problems = append(problems, problem)
	} else {
		t.MaxArchiveBytes = int64(mb) << 20
	}
	return t, problems
}
