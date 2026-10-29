package consumers

import (
	"context"
	"log"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
)

// SupportRecurrenceAlgorithmVersion is recorded on every corpus row and
// returned as evidence alongside any similarity result, so a future change to
// the embedding text shape or model is never silently conflated with today's.
const SupportRecurrenceAlgorithmVersion = "ticket-taxonomy-embed-v1"

// SupportRecurrenceCorpusService is the narrow surface the builder needs.
// *conversation.Service satisfies it.
type SupportRecurrenceCorpusService interface {
	DistinctOrgIDsWithActiveTickets(ctx context.Context) ([]string, error)
	ActiveTicketsForSupportRecurrenceCorpus(ctx context.Context, orgID string) ([]conversation.Ticket, error)
	UpsertSupportRecurrenceCorpusEntry(ctx context.Context, orgID, ticketID string, embedding []float32, algorithmVersion string, corpusWindowStart time.Time) error
	EvictStaleSupportRecurrenceCorpusEntries(ctx context.Context, orgID string, windowStart time.Time) error
	PurgeSupportRecurrenceCorpusByOrg(ctx context.Context, orgID string) error
}

// ZDROrgLookup is the narrow surface the builder needs from org-core.
// *clients.OrgCoreClient satisfies it.
type ZDROrgLookup interface {
	ZDREnabledOrgIDs(ctx context.Context) (map[string]bool, error)
}

// TextEmbedder is the narrow surface the builder needs from Data Plane v2.
// *clients.EmbeddingClient satisfies it.
type TextEmbedder interface {
	EmbedText(ctx context.Context, orgID, text string, zdr bool) ([]float32, error)
}

// SupportRecurrenceCorpusBuilder is a ticker sweep (modeled on
// OutboundIntentReconciler, not the NATS event-consumer scaffold — this is a
// periodic refresh, not a reaction to one event) that maintains a bounded,
// per-org semantic embedding corpus for the support-recurrence "similarity
// candidates" preview. It never builds anything for a ZDR-enabled org: ZDR
// status is checked live against org-core on every sweep, not cached or
// inferred from a NATS event — requirement #3 of the design gate is
// "ineligible", not "purged after the fact", so this is the primary
// enforcement point; SupportRecurrenceZDRPurgeConsumer is the reactive
// backstop for the gap between two sweeps.
type SupportRecurrenceCorpusBuilder struct {
	service    SupportRecurrenceCorpusService
	zdrLookup  ZDROrgLookup
	embedder   TextEmbedder
	interval   time.Duration
	windowSize time.Duration
	runTimeout time.Duration
	stop       chan struct{}
	done       chan struct{}
}

func NewSupportRecurrenceCorpusBuilder(
	service SupportRecurrenceCorpusService,
	zdrLookup ZDROrgLookup,
	embedder TextEmbedder,
	interval, windowSize time.Duration,
) *SupportRecurrenceCorpusBuilder {
	return &SupportRecurrenceCorpusBuilder{
		service:    service,
		zdrLookup:  zdrLookup,
		embedder:   embedder,
		interval:   interval,
		windowSize: windowSize,
		runTimeout: 2 * time.Minute,
		stop:       make(chan struct{}),
		done:       make(chan struct{}),
	}
}

func (b *SupportRecurrenceCorpusBuilder) Start(ctx context.Context) {
	go func() {
		defer close(b.done)
		b.run(ctx)
		ticker := time.NewTicker(b.interval)
		defer ticker.Stop()
		for {
			select {
			case <-ticker.C:
				b.run(ctx)
			case <-b.stop:
				return
			}
		}
	}()
}

func (b *SupportRecurrenceCorpusBuilder) Stop() {
	close(b.stop)
	<-b.done
}

func (b *SupportRecurrenceCorpusBuilder) run(ctx context.Context) {
	runCtx, cancel := context.WithTimeout(ctx, b.runTimeout)
	defer cancel()

	orgIDs, err := b.service.DistinctOrgIDsWithActiveTickets(runCtx)
	if err != nil {
		log.Printf("[cc-go/support-recurrence-corpus] list orgs failed: %v", err)
		return
	}
	if len(orgIDs) == 0 {
		return
	}

	zdrOrgIDs, err := b.zdrLookup.ZDREnabledOrgIDs(runCtx)
	if err != nil {
		// Fail closed: if ZDR status can't be verified this cycle, skip every
		// org rather than risk building a corpus for one that is actually
		// ZDR-enabled. The next sweep tries again.
		log.Printf("[cc-go/support-recurrence-corpus] ZDR lookup failed; skipping this sweep entirely: %v", err)
		return
	}

	windowStart := time.Now().Add(-b.windowSize)
	built, skipped := 0, 0
	for _, orgID := range orgIDs {
		if zdrOrgIDs[orgID] {
			skipped++
			continue
		}
		if err := b.buildOrg(runCtx, orgID, windowStart); err != nil {
			log.Printf("[cc-go/support-recurrence-corpus] build failed (org=%s): %v", orgID, err)
			continue
		}
		built++
	}
	log.Printf("[cc-go/support-recurrence-corpus] sweep complete: %d org(s) built, %d skipped (ZDR)", built, skipped)
}

func (b *SupportRecurrenceCorpusBuilder) buildOrg(ctx context.Context, orgID string, windowStart time.Time) error {
	tickets, err := b.service.ActiveTicketsForSupportRecurrenceCorpus(ctx, orgID)
	if err != nil {
		return err
	}
	for _, ticket := range tickets {
		text := supportRecurrenceEmbeddingText(ticket)
		if text == "" {
			continue
		}
		vector, err := b.embedder.EmbedText(ctx, orgID, text, false)
		if err != nil {
			log.Printf("[cc-go/support-recurrence-corpus] embed failed (org=%s, ticket=%s): %v", orgID, ticket.ID, err)
			continue
		}
		if err := b.service.UpsertSupportRecurrenceCorpusEntry(ctx, orgID, ticket.ID, vector, SupportRecurrenceAlgorithmVersion, windowStart); err != nil {
			log.Printf("[cc-go/support-recurrence-corpus] upsert failed (org=%s, ticket=%s): %v", orgID, ticket.ID, err)
		}
	}
	return b.service.EvictStaleSupportRecurrenceCorpusEntries(ctx, orgID, windowStart)
}

// supportRecurrenceEmbeddingText is the ONLY text this feature ever embeds —
// bounded taxonomy fields, never a customer transcript. Empty category/intent
// (an un-classified ticket) yields no usable signal, so the ticket is simply
// skipped rather than embedding an empty or near-empty string.
func supportRecurrenceEmbeddingText(ticket conversation.Ticket) string {
	text := ticket.Category
	if ticket.Intent != "" {
		if text != "" {
			text += " "
		}
		text += ticket.Intent
	}
	if ticket.WorkType != "" {
		if text != "" {
			text += " "
		}
		text += ticket.WorkType
	}
	return text
}
