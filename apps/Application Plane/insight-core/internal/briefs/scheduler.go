package briefs

import (
	"context"
	"fmt"
	"log"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/insight-core/internal/insights"
)

// briefWindow is the trailing window used both to discover active orgs and to
// scope each brief's overview. A daily brief summarises the last 24h of real
// recorded activity.
const briefWindow = 24 * time.Hour

// MetricsSource is the read surface the scheduler needs from insight-core. It is
// satisfied by *insights.Service. Org discovery and the per-org overview both
// resolve server-side from the recorded metric store — no client input.
type MetricsSource interface {
	OrgsWithRecentMetrics(ctx context.Context, window time.Duration) ([]string, error)
	Overview(ctx context.Context, query insights.OverviewQuery) (*insights.Overview, error)
}

// Scheduler is an unwired brief-assembly helper. It must not be started until an
// authoritative organization subscription resolves a Control user recipient.
// Its request key is deterministic per (org, UTC day) for contract tests.
type Scheduler struct {
	source   MetricsSource
	notifier NotificationClient
	interval time.Duration
	now      func() time.Time
}

// Option configures a Scheduler.
type Option func(*Scheduler)

// WithInterval overrides the tick interval (default 24h).
func WithInterval(d time.Duration) Option {
	return func(s *Scheduler) {
		if d > 0 {
			s.interval = d
		}
	}
}

// WithNow overrides the clock (mostly for tests).
func WithNow(now func() time.Time) Option {
	return func(s *Scheduler) {
		if now != nil {
			s.now = now
		}
	}
}

func NewScheduler(source MetricsSource, notifier NotificationClient, opts ...Option) *Scheduler {
	s := &Scheduler{
		source:   source,
		notifier: notifier,
		interval: briefWindow,
		now:      time.Now,
	}
	for _, opt := range opts {
		opt(s)
	}
	return s
}

// Run blocks, delivering one round of briefs immediately then on every tick
// until ctx is cancelled. Each round is best-effort: a per-org delivery failure
// is logged and the round continues; the org is retried on the next tick.
func (s *Scheduler) Run(ctx context.Context) {
	ticker := time.NewTicker(s.interval)
	defer ticker.Stop()

	s.deliverRound(ctx)
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			s.deliverRound(ctx)
		}
	}
}

// deliverRound discovers active orgs server-side and delivers each org's brief.
func (s *Scheduler) deliverRound(ctx context.Context) {
	orgIDs, err := s.source.OrgsWithRecentMetrics(ctx, briefWindow)
	if err != nil {
		log.Printf("[insight-core/briefs] discover orgs: %v", err)
		return
	}
	day := s.now().UTC().Format("2006-01-02")
	delivered := 0
	for _, orgID := range orgIDs {
		if err := s.deliverForOrg(ctx, orgID, day); err != nil {
			log.Printf("[insight-core/briefs] deliver org %s: %v", orgID, err)
			continue
		}
		delivered++
	}
	if len(orgIDs) > 0 {
		log.Printf("[insight-core/briefs] daily_brief delivered for %d/%d org(s) (day=%s)", delivered, len(orgIDs), day)
	}
}

// deliverForOrg assembles one org's brief for the given UTC day. RecipientID is
// currently an org placeholder and is not a valid notification-core recipient;
// production wiring is intentionally absent. Exposed for tests.
func (s *Scheduler) deliverForOrg(ctx context.Context, orgID, day string) error {
	to := s.now().UTC()
	from := to.Add(-briefWindow)
	overview, err := s.source.Overview(ctx, insights.OverviewQuery{
		OrgID: orgID,
		From:  &from,
		To:    &to,
	})
	if err != nil {
		return fmt.Errorf("overview: %w", err)
	}
	brief := AssembleBrief(overview)
	return s.notifier.Send(ctx, NotificationRequest{
		IdempotencyKey: BriefIdempotencyKey(orgID, day),
		RecipientID:    orgID,
		Type:           WorkflowDailyBrief,
		Payload:        brief.Payload(),
		Source:         "insight-core",
	})
}

// BriefIdempotencyKey is the stable per-(org, day) key. A re-tick or restart on
// the same UTC day resolves to the same key, so the brief is delivered at most
// once per org per day.
func BriefIdempotencyKey(orgID, day string) string {
	return "daily_brief_" + orgID + "_" + day
}
