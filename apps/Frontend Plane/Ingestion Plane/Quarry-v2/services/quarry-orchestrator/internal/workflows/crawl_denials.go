package workflows

import "github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/activities"

// Bounds of the denial summary of a crawl under scope (WS14.S3). A crawl of many
// pages must not grow the workflow history with every denied URL, so the summary
// keeps a count for each code, a few samples per code and a bounded set of
// URLs it has already sampled.
const (
	// denialMaxCodes bounds the distinct codes kept by name. A further code is
	// folded into denialOtherCode.
	denialMaxCodes = 12
	// denialSamplesPerCode is how many samples one code keeps.
	denialSamplesPerCode = 10
	// denialSamplesPerPage is how many samples one page's report may add. A report
	// that lists more is cut at this count.
	denialSamplesPerPage = 200
	// denialSeenCap bounds the URLs the summary has sampled. Past it, no new URL is
	// sampled (counts are still kept).
	denialSeenCap = 1000
	// denialOtherCode is the bucket for a code past denialMaxCodes.
	denialOtherCode = "other"
)

// denialAgg is the denial summary of one crawl: counts and samples, in the
// `denials` object of the terminal event.
type denialAgg struct {
	mode    string
	counts  map[string]uint32
	samples map[string][]activities.DenialSample
	seen    map[string]struct{}
}

func newDenialAgg(mode string) *denialAgg {
	return &denialAgg{
		mode:    mode,
		counts:  map[string]uint32{},
		samples: map[string][]activities.DenialSample{},
		seen:    map[string]struct{}{},
	}
}

// bucket returns the key a code is kept under: its own name while the summary
// has room for it, otherwise denialOtherCode.
func bucket[V any](m map[string]V, code string) string {
	if _, known := m[code]; known || len(m) < denialMaxCodes {
		return code
	}
	return denialOtherCode
}

// absorb folds one page's report into the summary.
func (d *denialAgg) absorb(report *activities.CrawlReport) {
	for code, n := range report.Denials.Counts {
		d.counts[bucket(d.counts, code)] += n
	}
	for i, sample := range report.Denials.Samples {
		if i >= denialSamplesPerPage {
			break
		}
		if _, dup := d.seen[sample.URL]; dup || len(d.seen) >= denialSeenCap {
			continue
		}
		key := bucket(d.samples, sample.Code)
		if len(d.samples[key]) >= denialSamplesPerCode {
			continue
		}
		d.seen[sample.URL] = struct{}{}
		d.samples[key] = append(d.samples[key], sample)
	}
}

// payload is the `denials` object of a terminal event. encoding/json sorts the
// map keys, so the payload is the same on every replay.
func (d *denialAgg) payload() map[string]any {
	return map[string]any{
		"mode":    d.mode,
		"counts":  d.counts,
		"samples": d.samples,
	}
}
