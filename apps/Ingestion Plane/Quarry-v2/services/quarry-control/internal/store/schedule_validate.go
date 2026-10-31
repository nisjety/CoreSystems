package store

import (
	"errors"
	"math"
	"net/url"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/cron"
)

// TargetKindChangeMonitor is the recurring change-monitoring schedule kind.
// Its cadence is preset-driven (see presetCron) rather than free-form cron.
const TargetKindChangeMonitor = "change_monitor"

// The three executable kinds of WS15.S1. Their definition lives in config.
const (
	TargetKindScrape = "scrape"
	TargetKindCrawl  = "crawl"
	TargetKindBatch  = "batch"
)

// EnvMinIntervalS overrides the cadence floor, in seconds, for scrape, crawl
// and batch schedules (default: 5 minutes for scrape, 15 for crawl and batch).
const EnvMinIntervalS = "QUARRY_SCHEDULE_MIN_INTERVAL_S"

const (
	maxSchedulePages  = 5000
	maxScheduleDepth  = 10
	maxBatchURLs      = 200
	maxCatchupWindowS = 86400
	maxJitterS        = 3600
)

// presetCron maps the fixed change-monitor cadences to literal 5-field cron
// expressions. cron.Validate rejects "@daily"/"@hourly" aliases and any
// non-5-field expression, so the presets MUST resolve to explicit fields.
// Times are in the scheduler's timezone (UTC in the orchestrator).
var presetCron = map[string]string{
	"hourly": "0 * * * *",
	"daily":  "0 9 * * *",
	"weekly": "0 9 * * 1",
}

// PresetCron returns the 5-field cron for a known change-monitor preset.
func PresetCron(preset string) (string, bool) {
	c, ok := presetCron[preset]
	return c, ok
}

var overlapPolicies = map[string]bool{"skip": true, "allow": true, "cancel": true}

// Validate checks a schedule definition before it is stored and fills the
// defaults a stored row carries (ID, timestamps, timezone, overlap, config).
// A failure is a *ValidationError with a machine Reason.
//
//   - change_monitor schedules are preset-driven: the caller sends a fixed
//     cadence and Validate maps it to a literal 5-field cron server-side.
//   - scrape and crawl need config.url (http or https, no credentials);
//     batch needs config.urls (1 to 200 such URLs). max_pages is 1 to 5000
//     and max_depth 0 to 10 when present.
//   - timezone is an IANA zone; overlap_policy is skip, allow or cancel;
//     catchup_window_s is 0 to 86400.
//   - scrape, crawl and batch have a cadence floor (cron.MinInterval over
//     three samples): 5 minutes for scrape, 15 for crawl and batch, or the
//     QUARRY_SCHEDULE_MIN_INTERVAL_S override.
//   - org_id is mandatory on every schedule; the handler takes it from the
//     edge-verified query, never from the body.
func (s *Schedule) Validate() error {
	now := time.Now().Unix()
	if s.OrgID == "" {
		return invalid("org_id_required", "org_id required")
	}
	s.ApplyDefaults(now)
	loc, err := loadTimezone(s.Timezone)
	if err != nil {
		return err
	}
	if !overlapPolicies[s.OverlapPolicy] {
		return invalid("invalid_overlap_policy", "overlap_policy must be skip, allow or cancel, got %q", s.OverlapPolicy)
	}
	if s.CatchupWindowS < 0 || s.CatchupWindowS > maxCatchupWindowS {
		return invalid("invalid_catchup_window", "catchup_window_s must be 0 to %d", maxCatchupWindowS)
	}
	if s.JitterS < 0 || s.JitterS > maxJitterS {
		return invalid("invalid_jitter", "jitter_s must be 0 to %d", maxJitterS)
	}

	switch s.TargetKind {
	case TargetKindChangeMonitor:
		if err := s.validateMonitor(); err != nil {
			return err
		}
	case TargetKindScrape, TargetKindCrawl, TargetKindBatch:
		if err := cron.Validate(s.Cron); err != nil {
			return invalid("invalid_cron", "%v", err)
		}
		if err := s.validateDefinition(); err != nil {
			return err
		}
		gap, err := cron.MinInterval(s.Cron, loc)
		if err != nil {
			return invalid("invalid_cron", "%v", err)
		}
		if floor := cadenceFloor(s.TargetKind); gap < floor {
			return invalid("cadence_too_frequent", "%s schedules fire at most every %s, this cron fires every %s", s.TargetKind, floor, gap)
		}
	default:
		return invalid("unsupported_schedule_kind", "target_kind must be scrape, crawl, batch or change_monitor, got %q", s.TargetKind)
	}

	if s.ID == "" {
		s.ID = quarrycontracts.NewID(quarrycontracts.KindSchedule)
	}
	return nil
}

// validateMonitor keeps the change_monitor contract: a preset becomes the cron,
// and a target_ref (the URL to watch) is required and must be set.
func (s *Schedule) validateMonitor() error {
	c, ok := PresetCron(s.Preset)
	if !ok {
		return invalid("invalid_preset", "change_monitor schedule requires a known preset (hourly|daily|weekly), got %q", s.Preset)
	}
	s.Cron = c
	if err := cron.Validate(s.Cron); err != nil {
		return invalid("invalid_cron", "%v", err)
	}
	if s.TargetRef == "" {
		return invalid("target_ref_required", "target_ref required")
	}
	return nil
}

// validateDefinition checks the config of a scrape, crawl or batch schedule and
// derives target_ref (the first URL) when the caller left it empty.
func (s *Schedule) validateDefinition() error {
	if s.TargetKind == TargetKindBatch {
		raw, _ := s.Config["urls"].([]any)
		if len(raw) < 1 || len(raw) > maxBatchURLs {
			return invalid("invalid_config", "config.urls must hold 1 to %d URLs", maxBatchURLs)
		}
		for i, v := range raw {
			str, _ := v.(string)
			if err := checkHTTPURL(str); err != nil {
				return invalid("invalid_config", "config.urls[%d]: %v", i, err)
			}
		}
		if first, _ := raw[0].(string); s.TargetRef == "" {
			s.TargetRef = first
		}
	} else {
		str, _ := s.Config["url"].(string)
		if err := checkHTTPURL(str); err != nil {
			return invalid("invalid_config", "config.url: %v", err)
		}
		if s.TargetRef == "" {
			s.TargetRef = str
		}
	}
	if err := checkConfigInt(s.Config, "max_pages", 1, maxSchedulePages); err != nil {
		return invalid("invalid_config", "config.max_pages: %v", err)
	}
	if err := checkConfigInt(s.Config, "max_depth", 0, maxScheduleDepth); err != nil {
		return invalid("invalid_config", "config.max_depth: %v", err)
	}
	return nil
}

func loadTimezone(name string) (*time.Location, error) {
	if name == "" {
		name = "UTC"
	}
	if name == "Local" {
		return nil, invalid("invalid_timezone", "timezone must be an IANA zone name, not Local")
	}
	loc, err := time.LoadLocation(name)
	if err != nil {
		return nil, invalid("invalid_timezone", "timezone %q is not an IANA zone", name)
	}
	return loc, nil
}

// cadenceFloor is the shortest gap allowed between fires, unless the env
// override is set to a positive number of seconds.
func cadenceFloor(kind string) time.Duration {
	if raw := strings.TrimSpace(os.Getenv(EnvMinIntervalS)); raw != "" {
		if n, err := strconv.Atoi(raw); err == nil && n > 0 {
			return time.Duration(n) * time.Second
		}
	}
	if kind == TargetKindScrape {
		return 5 * time.Minute
	}
	return 15 * time.Minute
}

func checkHTTPURL(raw string) error {
	if strings.TrimSpace(raw) == "" {
		return errors.New("required")
	}
	u, err := url.Parse(strings.TrimSpace(raw))
	if err != nil {
		return errors.New("not a URL")
	}
	if u.Scheme != "http" && u.Scheme != "https" {
		return errors.New("must be http or https")
	}
	if u.Host == "" {
		return errors.New("missing host")
	}
	if u.User != nil {
		return errors.New("must not carry credentials")
	}
	return nil
}

// checkConfigInt accepts a missing key. JSON numbers arrive as float64.
func checkConfigInt(cfg map[string]any, key string, lo, hi int64) error {
	v, present := cfg[key]
	if !present || v == nil {
		return nil
	}
	f, ok := v.(float64)
	if !ok || f != math.Trunc(f) {
		return errors.New("must be an integer")
	}
	if n := int64(f); n < lo || n > hi {
		return errors.New("out of range")
	}
	return nil
}
