// Package posture keeps quarry-control's per-org Zero Data Retention posture in
// step with Control Plane (X-9, the event route).
//
// org-core publishes controlplane.org.interactive_retention.enabled when an
// org's effective ZDR turns on: on the explicit toggle and, since X-9, on a
// plan change that turns it on. quarry-control consumes that event here and
// pauses the org's schedules and change monitors, which otherwise keep the ZDR
// stamp they were created with and keep fetching and storing baselines
// (D1-006, D2-192). There is no cross-tenant read scope for Quarry: the event
// is the only input, and every store call is scoped to the event's org.
//
// The consumer is behind QUARRY_CONTROL__ZDR_EVENT_CONSUMER=off|observe|enforce,
// default off (the subscriber is not started). observe logs would_pause for
// each schedule it would pause and writes nothing; enforce writes one
// schedule_skipped event with reason skipped_zdr per schedule and then pauses
// it (enabled=false). Nothing is ever resumed automatically: there is no
// ZDR-off event, and an operator or the org resumes a schedule by hand.
// Counsel decides prospective versus retroactive ZDR before enforce; pausing
// removes nothing already collected.
package posture

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/rs/zerolog"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

const (
	// ZDREnabledSubject is org-core's durable ZDR-enabled subject on the shared
	// CONTROLPLANE stream (the controlplane.> namespace).
	ZDREnabledSubject = "controlplane.org.interactive_retention.enabled"
	// DurableConsumerName is quarry-control's durable (and queue group) on that
	// subject. It must be pre-provisioned by audit-core's provisioner next to
	// conversation-core-interactive-retention; quarry-control binds, it never
	// creates consumers (same as gdpr.DurableConsumerName).
	DurableConsumerName = "quarry-control-interactive-retention"
	// controlSharedStream is the shared Control Plane stream (see gdpr).
	controlSharedStream = "CONTROLPLANE"

	// FlagEnv selects the consumer mode: off (default), observe or enforce.
	FlagEnv = "QUARRY_CONTROL__ZDR_EVENT_CONSUMER"

	// ReasonSkippedZDR is the reason on the schedule_skipped event written for
	// a schedule paused because its org turned ZDR on.
	ReasonSkippedZDR = "skipped_zdr"

	// listPageSize bounds one ListByOrg page.
	listPageSize = 200
)

// Mode is the consumer mode.
type Mode string

const (
	ModeOff     Mode = "off"
	ModeObserve Mode = "observe"
	ModeEnforce Mode = "enforce"
)

// ModeFromEnv reads FlagEnv. Unset or empty is off. A value that is not off,
// observe or enforce is off and reported as invalid, so the caller can log it:
// a typo must never enforce.
func ModeFromEnv(getenv func(string) string) (mode Mode, invalid bool) {
	switch strings.ToLower(strings.TrimSpace(getenv(FlagEnv))) {
	case "", "off", "0", "false", "no":
		return ModeOff, false
	case "observe", "shadow":
		return ModeObserve, false
	case "enforce":
		return ModeEnforce, false
	default:
		return ModeOff, true
	}
}

// Result counts what one event did (or, under observe, would have done).
type Result struct {
	Mode Mode
	// Enabled is the number of the org's schedules that were enabled.
	Enabled int
	// Monitors is how many of those are change monitors.
	Monitors int
	// AlreadyPaused is the number of the org's schedules that were already
	// disabled (a redelivery finds every schedule here).
	AlreadyPaused int
	// WouldPause is the observe count; Paused the enforce count.
	WouldPause int
	Paused     int
}

// zdrEnabledEvent is the org-core payload: {org_id, zdr: true} plus an
// optional cause (toggle events carry none). Unknown fields are ignored.
type zdrEnabledEvent struct {
	OrgID string `json:"org_id"`
	ZDR   bool   `json:"zdr"`
	Cause string `json:"cause,omitempty"`
}

// poisonEventError marks a payload no redelivery can fix (malformed JSON).
type poisonEventError struct{ err error }

func (e *poisonEventError) Error() string { return "posture: unprocessable event: " + e.err.Error() }
func (e *poisonEventError) Unwrap() error { return e.err }

// Consumer applies ZDR-enabled events to the org's schedules.
type Consumer struct {
	schedules store.SchedulesStore
	events    store.EventLog
	mode      Mode
	logger    zerolog.Logger
	now       func() time.Time
}

// NewConsumer builds a consumer over the control store.
func NewConsumer(db store.DB, mode Mode, logger zerolog.Logger) *Consumer {
	return &Consumer{
		schedules: db.Schedules(),
		events:    db.Events(),
		mode:      mode,
		logger:    logger,
		now:       time.Now,
	}
}

// Mode reports the consumer mode.
func (c *Consumer) Mode() Mode { return c.mode }

// Handle decodes one delivery and applies it. deliveryKey identifies the
// transition (the Nats-Msg-Id org-core sets, unique per transition); it makes
// the reason event of a redelivery collide with the first one instead of
// doubling it. A malformed payload is a *poisonEventError; an event with zdr
// absent or false, or no org_id, is a deliberate no-op (as Conversation Core
// treats it).
func (c *Consumer) Handle(data []byte, deliveryKey string) (Result, error) {
	var evt zdrEnabledEvent
	if err := json.Unmarshal(data, &evt); err != nil {
		return Result{Mode: c.mode}, &poisonEventError{err: err}
	}
	orgID := strings.TrimSpace(evt.OrgID)
	if !evt.ZDR || orgID == "" {
		return Result{Mode: c.mode}, nil
	}
	return c.MarkOrgZdr(orgID, evt.Cause, deliveryKey)
}

// MarkOrgZdr pauses (enforce) or reports (observe) every enabled schedule of
// orgID. It is idempotent: a schedule already disabled is counted and left
// alone, and the reason event of a schedule has a deterministic id per
// (org, schedule, deliveryKey), so a redelivery that finds the schedule still
// enabled (a crash between the event and the pause) rewrites nothing. Only
// org-scoped store methods are used: no other org's row is read or written.
func (c *Consumer) MarkOrgZdr(orgID, cause, deliveryKey string) (Result, error) {
	res := Result{Mode: c.mode}
	if c.mode != ModeObserve && c.mode != ModeEnforce {
		return res, nil
	}
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return res, nil
	}

	var targets []store.Schedule
	cursor := ""
	for {
		page, next := c.schedules.ListByOrg(orgID, listPageSize, cursor)
		for _, s := range page {
			if s.OrgID != orgID {
				// Defence in depth: ListByOrg is org-scoped in both stores.
				continue
			}
			if !s.Enabled {
				res.AlreadyPaused++
				continue
			}
			targets = append(targets, s)
		}
		if next == "" || len(page) == 0 {
			break
		}
		cursor = next
	}

	for _, s := range targets {
		res.Enabled++
		monitor := s.TargetKind == store.TargetKindChangeMonitor
		if monitor {
			res.Monitors++
		}
		if c.mode == ModeObserve {
			res.WouldPause++
			c.logger.Info().
				Str("org_id", orgID).
				Str("schedule_id", string(s.ID)).
				Str("target_kind", s.TargetKind).
				Str("cause", cause).
				Bool("would_pause", true).
				Msg("posture: org turned ZDR on; schedule would be paused (observe)")
			continue
		}
		if err := c.writeReason(s, cause, deliveryKey); err != nil {
			return res, err
		}
		if err := c.schedules.UpdateEnabledByOrg(s.ID, orgID, false); err != nil {
			if errors.Is(err, store.ErrNotFound) {
				// Deleted since the list: nothing left to pause.
				continue
			}
			return res, fmt.Errorf("posture: pause schedule %s: %w", s.ID, err)
		}
		res.Paused++
	}

	if res.Enabled > 0 || res.AlreadyPaused > 0 {
		c.logger.Info().
			Str("org_id", orgID).
			Str("mode", string(c.mode)).
			Str("cause", cause).
			Int("enabled", res.Enabled).
			Int("monitors", res.Monitors).
			Int("already_paused", res.AlreadyPaused).
			Int("would_pause", res.WouldPause).
			Int("paused", res.Paused).
			Msg("posture: ZDR-enabled event applied")
	}
	return res, nil
}

// writeReason records why the schedule stopped: a schedule_skipped event with
// reason skipped_zdr, outside any run or job. The payload is content-free (no
// URL, no org id: the schedule id is enough and an erased org leaves no id
// behind in it). A conflict means this transition already wrote it.
func (c *Consumer) writeReason(s store.Schedule, cause, deliveryKey string) error {
	payload := map[string]any{
		"schedule_id": string(s.ID),
		"target_kind": s.TargetKind,
		"reason":      ReasonSkippedZDR,
	}
	if cause != "" {
		payload["cause"] = cause
	}
	digest := reasonDigest(s.OrgID, string(s.ID), deliveryKey)
	evt := quarrycontracts.Event{
		EventID:        reasonEventID(digest),
		Type:           quarrycontracts.EvtScheduleSkipped,
		Timestamp:      c.now().UTC(),
		Payload:        payload,
		IdempotencyKey: "zdr-pause:" + string(s.ID) + ":" + digest[:16],
	}
	if _, err := c.events.AppendNext(evt); err != nil {
		if errors.Is(err, store.ErrConflict) {
			return nil
		}
		return fmt.Errorf("posture: write %s reason for schedule %s: %w", ReasonSkippedZDR, s.ID, err)
	}
	return nil
}

func reasonDigest(orgID, scheduleID, deliveryKey string) string {
	sum := sha256.Sum256([]byte(orgID + "\x00" + scheduleID + "\x00" + deliveryKey))
	return hex.EncodeToString(sum[:])
}

func reasonEventID(digest string) quarrycontracts.ID {
	return quarrycontracts.ID(string(quarrycontracts.KindEvent) + "zdr" + digest[:26])
}
