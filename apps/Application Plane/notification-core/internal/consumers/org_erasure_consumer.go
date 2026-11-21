// Package consumers: organization erasure. org-core (Control Plane) announces a GDPR erasure of an organization on the
// shared Control-Plane stream, both from its explicit hard-delete path and from its 30-day auto-purge cron. This
// subscriber erases every notification-core record of that organization. In novu mode its provider-side subscribers and
// messages go first. Disabled mode has no provider records, so it erases the local tables alone. Then, in one
// transaction, every organization-keyed table of the notification store goes, Web Push subscriptions included.
//
// The same subject carries the per-user erasure: user-core publishes subject_type "user" for a hard delete and
// "user_anonymize" for an anonymize. This subscriber erases that user's notification-core records too, in every
// organization, with the same order: in novu mode the user's provider-side subscribers and messages go first, then one
// transaction removes the user's rows everywhere. See processUser.
//
// Modeled on OrgDeletionSubscriber (org_deletion_consumer.go): the same struct shape, the same durable bind on the
// pre-provisioned CONTROLPLANE consumer, and the same Start/Stop lifecycle. The NATS handler is kept apart from the
// testable process step, so the ack-or-retry policy is tested without a broker.
package consumers

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"sort"
	"strings"
	"sync"

	"github.com/nats-io/nats.go"
)

const (
	// SubjectGDPRErasureRequested is the platform-wide GDPR erasure fan-out subject. org-core publishes it on the shared
	// Control-Plane bus. The same subject also carries subject_type "user" and "user_anonymize" for the per-user erasure
	// fan-out, which this subscriber erases as well (see processUser).
	SubjectGDPRErasureRequested = "verevon.gdpr.erasure.requested"

	// orgErasureDurable is the server-side consumer on controlSharedStream that this subscriber binds to. The Control
	// Plane provisions it (F-f05-11). Until it exists, the bind fails and the caller retries in the background.
	orgErasureDurable = "notification-core-org-erasure"

	// erasureSubjectOrganization is the subject_type of an organization erasure.
	erasureSubjectOrganization = "organization"

	// erasureSubjectUser is the subject_type of a per-user hard delete, and erasureSubjectUserAnonymize of a per-user
	// anonymize. Both erase the user's notification-core records the same way.
	erasureSubjectUser          = "user"
	erasureSubjectUserAnonymize = "user_anonymize"
)

// orgErasureEvent is the wire shape of verevon.gdpr.erasure.requested:
// {"subject_type":"organization","subject_id":"<org_id>","org_id":"<org_id>","requested_by":"<actor_id>","ts":"<RFC3339Nano>"}.
type orgErasureEvent struct {
	SubjectType string `json:"subject_type"`
	SubjectID   string `json:"subject_id"`
	OrgID       string `json:"org_id"`
	RequestedBy string `json:"requested_by"`
	Timestamp   string `json:"ts"`
}

// OrgPurger erases one organization's records from one local store or one provider. It returns the rows removed per
// table, so the log can carry counts and never content. A replayed erasure removes zero rows. The notification
// repository satisfies it directly, covering every organization-keyed table in one transaction, and the provider-side
// eraser satisfies it directly.
type OrgPurger interface {
	PurgeOrganization(ctx context.Context, orgID string) (map[string]int64, error)
}

// UserPurger erases one user's records, in every organization, from one local store or one provider. It returns the rows
// removed per table, with the same content-free contract as OrgPurger. A replayed erasure removes zero rows. The notification
// repository satisfies it directly, and the provider-side eraser satisfies it directly.
type UserPurger interface {
	PurgeUser(ctx context.Context, userID string) (map[string]int64, error)
}

// orgErasureOutcome is the ack-or-retry decision for one message.
type orgErasureOutcome int

const (
	orgErasureAck orgErasureOutcome = iota
	orgErasureRetry
)

// OrgErasureSubscriber runs every OrgPurger for an erased organization. Provider-side steps run first and gate the local
// purgers: a local purge removes the membership rows that a provider step reads to find the provider identities it must
// erase, so no local store is purged until every provider step has succeeded. Among the local purgers, one that fails
// does not stop the others, and any failure makes the whole message retry. That is safe because every purge is
// idempotent.
type OrgErasureSubscriber struct {
	js        nats.JetStreamContext
	providers []OrgPurger
	purgers   []OrgPurger

	// userProviders and userPurgers are the per-user steps. They keep the organization steps' order and gate.
	userProviders []UserPurger
	userPurgers   []UserPurger

	mu  sync.Mutex
	sub *nats.Subscription
}

// NewOrgErasureSubscriber wires the subscriber over every store that holds organization-keyed rows.
func NewOrgErasureSubscriber(js nats.JetStreamContext, purgers ...OrgPurger) *OrgErasureSubscriber {
	return &OrgErasureSubscriber{js: js, purgers: purgers}
}

// erasureNovuDeliveryMode is the delivery mode in which Novu holds the provider-side records that an erasure removes.
const erasureNovuDeliveryMode = "novu"

// NewOrgErasureSubscriberForMode wires the subscriber for one delivery mode. In novu mode the provider-side eraser runs
// first and gates every local store. In any other mode the provider step is left out and the subscriber purges the local
// stores alone. The disabled Novu adapter has no client, so a provider step there would fail every message and retry it
// forever, and that mode holds no provider record to erase. The one line logged here names the mode choice and carries no
// organization or content.
func NewOrgErasureSubscriberForMode(js nats.JetStreamContext, deliveryMode string, provider OrgPurger, purgers ...OrgPurger) *OrgErasureSubscriber {
	subscriber := NewOrgErasureSubscriber(js, purgers...)
	if deliveryMode == erasureNovuDeliveryMode {
		return subscriber.WithProviderSteps(provider)
	}
	log.Printf("consumers/org-erasure: provider-side erasure is off because the delivery mode is not novu, so only the local rows are erased")
	return subscriber
}

// WithProviderSteps sets the provider-side erasure steps. They run before every local purger, and a failed step retries
// the whole message with the local rows still in place, so the retry can find the memberships again. Call it before
// Start.
func (s *OrgErasureSubscriber) WithProviderSteps(steps ...OrgPurger) *OrgErasureSubscriber {
	s.providers = append([]OrgPurger(nil), steps...)
	return s
}

// WithUserErasure sets the per-user erasure steps. In novu mode the provider-side eraser runs first and gates every local
// purger, as it does for an organization. In any other mode the provider step is left out, for the same reason
// NewOrgErasureSubscriberForMode leaves it out: that mode holds no provider record to erase. Call it before Start.
func (s *OrgErasureSubscriber) WithUserErasure(deliveryMode string, provider UserPurger, purgers ...UserPurger) *OrgErasureSubscriber {
	s.userProviders = nil
	if deliveryMode == erasureNovuDeliveryMode {
		s.userProviders = []UserPurger{provider}
	}
	s.userPurgers = append([]UserPurger(nil), purgers...)
	return s
}

// Start binds the durable on the shared Control-Plane stream. It is idempotent, and it returns an error only when the
// bind itself fails. A missing broker or an empty purger list is a logged no-op, the same fail-open shape as the
// org-deletion subscriber.
func (s *OrgErasureSubscriber) Start(ctx context.Context) error {
	if s == nil {
		return nil
	}
	if s.js == nil {
		log.Printf("consumers/org-erasure: JetStream not configured, skipping subscription")
		return nil
	}
	if len(s.purgers) == 0 && len(s.providers) == 0 && len(s.userPurgers) == 0 && len(s.userProviders) == 0 {
		log.Printf("consumers/org-erasure: no store to purge, skipping subscription")
		return nil
	}

	s.mu.Lock()
	defer s.mu.Unlock()
	if s.sub != nil {
		return nil
	}
	sub, err := s.js.QueueSubscribe(
		SubjectGDPRErasureRequested,
		orgErasureDurable,
		s.handle(ctx),
		nats.Bind(controlSharedStream, orgErasureDurable),
		nats.ManualAck(),
	)
	if err != nil {
		return fmt.Errorf("subscribe %s: %w", SubjectGDPRErasureRequested, err)
	}
	s.sub = sub
	log.Printf("consumers/org-erasure: subscribed to %s", SubjectGDPRErasureRequested)
	return nil
}

// Stop drains the subscription. It is safe to call more than once.
func (s *OrgErasureSubscriber) Stop() {
	if s == nil {
		return
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.sub == nil {
		return
	}
	if err := s.sub.Drain(); err != nil {
		log.Printf("consumers/org-erasure: drain error: %v", err)
	}
	s.sub = nil
}

func (s *OrgErasureSubscriber) handle(ctx context.Context) nats.MsgHandler {
	return func(msg *nats.Msg) {
		if s.processPayload(ctx, msg.Data) == orgErasureRetry {
			if err := msg.Nak(); err != nil {
				log.Printf("consumers/org-erasure: nak failed: %v", err)
			}
			return
		}
		if err := msg.Ack(); err != nil {
			log.Printf("consumers/org-erasure: ack failed: %v", err)
		}
	}
}

// processPayload decodes one erasure-requested message. A payload that does not decode is poison: nothing in it can
// be acted on, so it is acknowledged, and the log names no content. The byte count is the only detail logged.
func (s *OrgErasureSubscriber) processPayload(ctx context.Context, data []byte) orgErasureOutcome {
	var event orgErasureEvent
	if err := json.Unmarshal(data, &event); err != nil {
		log.Printf("consumers/org-erasure: malformed erasure event (%d bytes) acknowledged without a purge", len(data))
		return orgErasureAck
	}
	return s.process(ctx, event)
}

// process purges one organization when the event names one, and one user when it is a per-user erasure. Any other
// subject_type is outside this subscriber's authority and acks as a definitive no-op. It is the testable core and needs no
// broker.
func (s *OrgErasureSubscriber) process(ctx context.Context, event orgErasureEvent) orgErasureOutcome {
	subjectType := strings.TrimSpace(event.SubjectType)
	if subjectType == erasureSubjectUser || subjectType == erasureSubjectUserAnonymize {
		return s.processUser(ctx, event)
	}
	if subjectType != erasureSubjectOrganization {
		return orgErasureAck
	}
	orgID := strings.TrimSpace(event.OrgID)
	if orgID == "" {
		orgID = strings.TrimSpace(event.SubjectID)
	}
	if orgID == "" {
		log.Printf("consumers/org-erasure: organization erasure without an org id acknowledged without a purge")
		return orgErasureAck
	}

	removed := make(map[string]int64)
	// The provider-side steps come first, and a failure stops the message before any local store is touched. The
	// memberships that name the provider identities are local rows, and a local purge would remove them.
	for _, step := range s.providers {
		counts, err := step.PurgeOrganization(ctx, orgID)
		for table, count := range counts {
			removed[table] += count
		}
		if err != nil {
			log.Printf("consumers/org-erasure: provider-side erasure failed for organization %s, local rows kept for retry: %v", orgID, err)
			return orgErasureRetry
		}
	}

	var failures []error
	for _, purger := range s.purgers {
		counts, err := purger.PurgeOrganization(ctx, orgID)
		for table, count := range counts {
			removed[table] += count
		}
		if err != nil {
			failures = append(failures, err)
		}
	}
	if len(failures) > 0 {
		log.Printf("consumers/org-erasure: purge failed for organization %s, will retry: %v", orgID, errors.Join(failures...))
		return orgErasureRetry
	}
	log.Printf("consumers/org-erasure: erased notification-core records for organization %s (%s)", orgID, formatRemovedCounts(removed))
	return orgErasureAck
}

// processUser erases one user across every organization. user-core publishes one event per organization the user belongs to,
// and each names the user in subject_id. The Auth erasure behind them is global to the user, so the purge is keyed by the
// user alone. The event's org_id names only the fan-out child that carried it, and the first child to arrive erases the user
// everywhere. A replayed child removes zero rows and still acks. Unlike an organization event, a user event never falls back
// to org_id, because an organization id is not a user id. The log carries the counts and never the user id.
func (s *OrgErasureSubscriber) processUser(ctx context.Context, event orgErasureEvent) orgErasureOutcome {
	userID := strings.TrimSpace(event.SubjectID)
	if userID == "" {
		log.Printf("consumers/org-erasure: user erasure without a user id acknowledged without a purge")
		return orgErasureAck
	}

	removed := make(map[string]int64)
	// The provider-side step comes first, and a failure stops the message before any local store is touched. The memberships
	// that name the provider identities are local rows, and a local purge would remove them.
	for _, step := range s.userProviders {
		counts, err := step.PurgeUser(ctx, userID)
		for table, count := range counts {
			removed[table] += count
		}
		if err != nil {
			log.Printf("consumers/org-erasure: provider-side erasure failed for one user, local rows kept for retry: %v", err)
			return orgErasureRetry
		}
	}

	var failures []error
	for _, purger := range s.userPurgers {
		counts, err := purger.PurgeUser(ctx, userID)
		for table, count := range counts {
			removed[table] += count
		}
		if err != nil {
			failures = append(failures, err)
		}
	}
	if len(failures) > 0 {
		log.Printf("consumers/org-erasure: purge failed for one user, will retry: %v", errors.Join(failures...))
		return orgErasureRetry
	}
	log.Printf("consumers/org-erasure: erased notification-core records for one user (%s)", formatRemovedCounts(removed))
	return orgErasureAck
}

// formatRemovedCounts renders the per-table counts in table order, so the log line is stable from one purge to the next.
func formatRemovedCounts(removed map[string]int64) string {
	tables := make([]string, 0, len(removed))
	for table := range removed {
		tables = append(tables, table)
	}
	sort.Strings(tables)
	parts := make([]string, 0, len(tables))
	for _, table := range tables {
		parts = append(parts, fmt.Sprintf("%s=%d", table, removed[table]))
	}
	return strings.Join(parts, " ")
}
