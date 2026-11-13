package eventtrigger

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"strconv"
	"strings"
	"time"

	"github.com/triodelab/model-plane/services/capability-core/internal/cron"
)

// The statuses a fire row holds. Only a pending fire is decided; the other two are final.
const (
	StatusPending    = "pending"
	StatusAuthorized = "authorized"
	StatusRefused    = "refused"
)

// The outcomes a settled fire records. Authorized is the only one that leaves a fire open for a run.
const (
	OutcomeAuthorized       = "authorized"
	OutcomeTriggerNotActive = "trigger_not_active"
	OutcomeTriggerBudget    = "trigger_budget_reached"
	OutcomeOrgQuota         = "org_quota_reached"
	OutcomeAuthorityFailed  = "authority_unavailable"
)

// The outcomes the wake gate records (W4-F T5). Control is never asked about a fire that ends in either: a closed gate is
// a refusal, and a payload that cannot be read within the attempts is a refusal too.
const (
	// OutcomeWakeGateClosed marks a fire whose webhook payload its trigger's wake filter does not match.
	OutcomeWakeGateClosed = "wake_gate_closed"
	// OutcomeWakePayloadUnavailable marks a fire whose webhook payload could not be read within its attempts.
	OutcomeWakePayloadUnavailable = "wake_payload_unavailable"
)

// DefaultTriggerFiresPerHour bounds how many fires one trigger may be authorised for in an hour.
const DefaultTriggerFiresPerHour = 12

// DefaultOrgFiresPerHour bounds how many fires one org may be authorised for in an hour.
const DefaultOrgFiresPerHour = 120

// DefaultMaxAuthAttempts is how many times a fire is asked of Control before it is refused.
const DefaultMaxAuthAttempts = 3

// fireWindow is the span the budgets count authorised fires over.
const fireWindow = time.Hour

// Fire is one pending fire, with the trigger fields a decision reads. WakeFilter is the trigger's stored wake filter as
// JSON, or nil for a trigger with none.
type Fire struct {
	ID             string
	TriggerID      string
	OrgID          string
	WebhookEventID string
	ConnectionID   string
	SpaceRef       string
	MemberUserID   string
	TriggerStatus  string
	Attempts       int
	WakeFilter     json.RawMessage
}

// Limits are the budgets a fire is decided against. Each counts authorised fires over fireWindow.
type Limits struct {
	TriggerPerHour int
	OrgPerHour     int
	MaxAttempts    int
}

// Counts are the authorised fires a decision sees in the window: the trigger's and the whole org's.
type Counts struct {
	Trigger int
	Org     int
}

// Settlement is how a decided fire is written back. A pending settlement only records the attempt.
type Settlement struct {
	Status   string
	Outcome  string
	Attempts int
}

// LimitsFromEnv reads the per-trigger and per-org hourly budgets. A missing, zero or unparsable value takes its
// default, so a typo can never remove a budget.
func LimitsFromEnv(getenv func(string) string) Limits {
	return Limits{
		TriggerPerHour: positiveEnv(getenv, "EVENT_TRIGGER_FIRES_PER_TRIGGER_HOUR", DefaultTriggerFiresPerHour),
		OrgPerHour:     positiveEnv(getenv, "EVENT_TRIGGER_FIRES_PER_ORG_HOUR", DefaultOrgFiresPerHour),
		MaxAttempts:    DefaultMaxAuthAttempts,
	}
}

func positiveEnv(getenv func(string) string, name string, fallback int) int {
	if getenv == nil {
		return fallback
	}
	value, err := strconv.Atoi(strings.TrimSpace(getenv(name)))
	if err != nil || value <= 0 {
		return fallback
	}
	return value
}

// withDefaults fills any zero budget with its default. A zero budget would refuse every fire.
func (l Limits) withDefaults() Limits {
	if l.TriggerPerHour <= 0 {
		l.TriggerPerHour = DefaultTriggerFiresPerHour
	}
	if l.OrgPerHour <= 0 {
		l.OrgPerHour = DefaultOrgFiresPerHour
	}
	if l.MaxAttempts <= 0 {
		l.MaxAttempts = DefaultMaxAuthAttempts
	}
	return l
}

// refusalFor names the reason a pending fire is refused before Control is asked, or is empty when it may be asked.
// The trigger is checked first: a trigger paused or disabled since its event never fires.
func refusalFor(fire Fire, counts Counts, limits Limits) string {
	switch {
	case fire.TriggerStatus != "active":
		return OutcomeTriggerNotActive
	case counts.Trigger >= limits.TriggerPerHour:
		return OutcomeTriggerBudget
	case counts.Org >= limits.OrgPerHour:
		return OutcomeOrgQuota
	default:
		return ""
	}
}

// decide settles one pending fire. A refused fire never reaches Control. A fire whose trigger carries a wake filter must
// also pass the wake gate first: a closed gate refuses it, and a payload that cannot be read leaves it pending, as an
// authority error does. An authority error leaves the fire pending for a later sweep until it has used its attempts, and
// then refuses it, so a Control outage cannot hold a fire open forever. The error is returned for the log: a refusal and
// an outage look the same from here. A nil gate is the switch being off: stored filters are then ignored.
func decide(ctx context.Context, authority cron.FireAuthorizer, gate wakeGate, fire Fire, counts Counts, limits Limits) (Settlement, error) {
	if refusal := refusalFor(fire, counts, limits); refusal != "" {
		return Settlement{Status: StatusRefused, Outcome: refusal, Attempts: fire.Attempts}, nil
	}
	if gate != nil && len(fire.WakeFilter) > 0 {
		open, err := gate.admit(ctx, fire)
		if err != nil {
			attempts := fire.Attempts + 1
			if attempts >= limits.MaxAttempts {
				return Settlement{Status: StatusRefused, Outcome: OutcomeWakePayloadUnavailable, Attempts: attempts}, err
			}
			return Settlement{Status: StatusPending, Attempts: attempts}, err
		}
		if !open {
			return Settlement{Status: StatusRefused, Outcome: OutcomeWakeGateClosed, Attempts: fire.Attempts}, nil
		}
	}
	attempts := fire.Attempts + 1
	if err := authority.AuthorizeFire(ctx, intentFor(fire)); err != nil {
		if attempts >= limits.MaxAttempts {
			return Settlement{Status: StatusRefused, Outcome: OutcomeAuthorityFailed, Attempts: attempts}, err
		}
		return Settlement{Status: StatusPending, Attempts: attempts}, err
	}
	return Settlement{Status: StatusAuthorized, Outcome: OutcomeAuthorized, Attempts: attempts}, nil
}

// intentFor is the non-secret intent Control decides a fire on. The trigger record plays the part a schedule plays in
// the cron rule: the schedule id is the trigger id and the fire key is the webhook event id, so each event is one fire
// of one trigger and cannot be replayed as another.
func intentFor(fire Fire) cron.FireIntent {
	return cron.FireIntent{
		OrgID:          fire.OrgID,
		SpaceRef:       fire.SpaceRef,
		SubjectID:      fire.MemberUserID,
		ScheduleID:     fire.TriggerID,
		FireKey:        fire.WebhookEventID,
		TemplateDigest: triggerDigest(fire),
		IdempotencyKey: fire.TriggerID + ":" + fire.WebhookEventID,
	}
}

// triggerDigest commits a fire to the trigger's own binding: its id, connection, Space, member and event type. The
// label is not part of it, so renaming a trigger does not change what Control decided on.
func triggerDigest(fire Fire) string {
	sum := sha256.Sum256([]byte("model.event_trigger.fire\x00v1\x00" + fire.TriggerID + "\x00" + fire.ConnectionID +
		"\x00" + fire.SpaceRef + "\x00" + fire.MemberUserID + "\x00" + "webhook_received"))
	return "sha256:" + hex.EncodeToString(sum[:])
}
