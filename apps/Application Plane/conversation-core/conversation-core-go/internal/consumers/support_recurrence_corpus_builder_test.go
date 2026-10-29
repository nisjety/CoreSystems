package consumers

import (
	"context"
	"errors"
	"sync"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
)

type fakeSupportRecurrenceCorpusService struct {
	mu               sync.Mutex
	orgIDs           []string
	ticketsByOrg     map[string][]conversation.Ticket
	upserts          []string // "orgID:ticketID"
	evictedOrgs      []string
	distinctOrgsErr  error
	activeTicketsErr error
}

func (f *fakeSupportRecurrenceCorpusService) DistinctOrgIDsWithActiveTickets(_ context.Context) ([]string, error) {
	if f.distinctOrgsErr != nil {
		return nil, f.distinctOrgsErr
	}
	return f.orgIDs, nil
}

func (f *fakeSupportRecurrenceCorpusService) ActiveTicketsForSupportRecurrenceCorpus(_ context.Context, orgID string) ([]conversation.Ticket, error) {
	if f.activeTicketsErr != nil {
		return nil, f.activeTicketsErr
	}
	return f.ticketsByOrg[orgID], nil
}

func (f *fakeSupportRecurrenceCorpusService) UpsertSupportRecurrenceCorpusEntry(_ context.Context, orgID, ticketID string, _ []float32, _ string, _ time.Time) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.upserts = append(f.upserts, orgID+":"+ticketID)
	return nil
}

func (f *fakeSupportRecurrenceCorpusService) EvictStaleSupportRecurrenceCorpusEntries(_ context.Context, orgID string, _ time.Time) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.evictedOrgs = append(f.evictedOrgs, orgID)
	return nil
}

func (f *fakeSupportRecurrenceCorpusService) PurgeSupportRecurrenceCorpusByOrg(_ context.Context, _ string) error {
	return nil
}

type fakeZDROrgLookup struct {
	zdrOrgIDs map[string]bool
	err       error
}

func (f *fakeZDROrgLookup) ZDREnabledOrgIDs(_ context.Context) (map[string]bool, error) {
	if f.err != nil {
		return nil, f.err
	}
	return f.zdrOrgIDs, nil
}

type fakeTextEmbedder struct {
	mu    sync.Mutex
	calls []string // "orgID:text"
}

func (f *fakeTextEmbedder) EmbedText(_ context.Context, orgID, text string, _ bool) ([]float32, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.calls = append(f.calls, orgID+":"+text)
	return []float32{0.1, 0.2}, nil
}

func TestSupportRecurrenceCorpusBuilder_SkipsZDROrgsEntirely(t *testing.T) {
	service := &fakeSupportRecurrenceCorpusService{
		orgIDs: []string{"org-zdr", "org-plain"},
		ticketsByOrg: map[string][]conversation.Ticket{
			"org-zdr":   {{ID: "t1", Category: "billing", Intent: "refund"}},
			"org-plain": {{ID: "t2", Category: "billing", Intent: "refund"}},
		},
	}
	zdrLookup := &fakeZDROrgLookup{zdrOrgIDs: map[string]bool{"org-zdr": true}}
	embedder := &fakeTextEmbedder{}
	builder := NewSupportRecurrenceCorpusBuilder(service, zdrLookup, embedder, time.Hour, 90*24*time.Hour)

	builder.run(t.Context())

	for _, call := range embedder.calls {
		if len(call) >= len("org-zdr") && call[:len("org-zdr")] == "org-zdr" {
			t.Fatalf("embedded text for a ZDR-enabled org: %q", call)
		}
	}
	if len(embedder.calls) != 1 || embedder.calls[0] != "org-plain:billing refund" {
		t.Fatalf("embedder calls = %#v, want exactly one call for org-plain", embedder.calls)
	}
	if len(service.upserts) != 1 || service.upserts[0] != "org-plain:t2" {
		t.Fatalf("upserts = %#v, want exactly one for org-plain:t2", service.upserts)
	}
	for _, orgID := range service.evictedOrgs {
		if orgID == "org-zdr" {
			t.Fatalf("evicted a ZDR-enabled org's corpus during a build sweep; that org should never have been built or touched here")
		}
	}
}

func TestSupportRecurrenceCorpusBuilder_ZDRLookupFailureSkipsEntireSweep(t *testing.T) {
	service := &fakeSupportRecurrenceCorpusService{
		orgIDs: []string{"org-1"},
		ticketsByOrg: map[string][]conversation.Ticket{
			"org-1": {{ID: "t1", Category: "billing"}},
		},
	}
	zdrLookup := &fakeZDROrgLookup{err: errors.New("org-core unavailable")}
	embedder := &fakeTextEmbedder{}
	builder := NewSupportRecurrenceCorpusBuilder(service, zdrLookup, embedder, time.Hour, 90*24*time.Hour)

	builder.run(t.Context())

	if len(embedder.calls) != 0 {
		t.Fatalf("embedder calls = %#v, want none when ZDR status can't be verified", embedder.calls)
	}
	if len(service.upserts) != 0 {
		t.Fatalf("upserts = %#v, want none when ZDR status can't be verified", service.upserts)
	}
}

func TestSupportRecurrenceCorpusBuilder_SkipsTicketsWithNoUsableTaxonomyText(t *testing.T) {
	service := &fakeSupportRecurrenceCorpusService{
		orgIDs: []string{"org-1"},
		ticketsByOrg: map[string][]conversation.Ticket{
			"org-1": {{ID: "unclassified"}},
		},
	}
	zdrLookup := &fakeZDROrgLookup{zdrOrgIDs: map[string]bool{}}
	embedder := &fakeTextEmbedder{}
	builder := NewSupportRecurrenceCorpusBuilder(service, zdrLookup, embedder, time.Hour, 90*24*time.Hour)

	builder.run(t.Context())

	if len(embedder.calls) != 0 {
		t.Fatalf("embedder calls = %#v, want none for a ticket with no category/intent/work_type", embedder.calls)
	}
}

func TestSupportRecurrenceEmbeddingText(t *testing.T) {
	cases := []struct {
		name   string
		ticket conversation.Ticket
		want   string
	}{
		{"all fields", conversation.Ticket{Category: "billing", Intent: "refund", WorkType: "customer_case"}, "billing refund customer_case"},
		{"category only", conversation.Ticket{Category: "billing"}, "billing"},
		{"nothing", conversation.Ticket{}, ""},
	}
	for _, tc := range cases {
		if got := supportRecurrenceEmbeddingText(tc.ticket); got != tc.want {
			t.Errorf("%s: supportRecurrenceEmbeddingText() = %q, want %q", tc.name, got, tc.want)
		}
	}
}
