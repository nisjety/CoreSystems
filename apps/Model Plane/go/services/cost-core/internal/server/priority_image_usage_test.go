package server

import (
	"context"
	"math"
	"testing"

	"github.com/triodelab/model-plane/services/cost-core/internal/ledger"
	"github.com/triodelab/model-plane/services/cost-core/internal/pricing"
)

// FU-24: RecordUsage applies the priority-tier multiplier once, only to a cost it prices itself
// from the catalogue. W4-IMG-2: RecordImageUsage records an image with no catalogue price at no
// image cost, and never prices a subscription entry.

func usageCost(t *testing.T, srv *Server, user string) float64 {
	t.Helper()
	u, err := srv.ledger.GetUsage(context.Background(), "org1", user)
	if err != nil {
		t.Fatalf("GetUsage(%s): %v", user, err)
	}
	return u.TotalCostUSD
}

func TestRecordUsageAppliesThePriorityMultiplierToACatalogueCost(t *testing.T) {
	srv := NewServer(ledger.NewStore())
	ctx := context.Background()
	entry := func(user string, priority bool, cost float64, basis string) ledger.Entry {
		return ledger.Entry{OrgID: "org1", UserID: user, Model: "gpt-5.6-terra", InputTokens: 10_000, OutputTokens: 2_000,
			CostUSD: cost, CostBasis: basis, PriorityTier: priority, IdempotencyKey: user}
	}
	list := srv.pricing.CostWithCache("gpt-5.6-terra", 10_000, 2_000, 0, 0)
	if list <= 0 {
		t.Fatalf("catalogue price %v; the test needs a priced model", list)
	}
	for _, e := range []ledger.Entry{
		entry("standard", false, 0, ledger.CostBasisMetered),
		entry("priority", true, 0, ledger.CostBasisMetered),
		entry("priority-unknown-basis", true, 0, ledger.CostBasisUnknown),
		entry("priority-stated", true, 0.5, ledger.CostBasisMetered),
		entry("priority-subscription", true, 0, ledger.CostBasisSubscription),
		entry("priority-customer", true, 0, ledger.CostBasisCustomerConnection),
	} {
		if err := srv.RecordUsage(ctx, e); err != nil {
			t.Fatalf("RecordUsage(%s): %v", e.UserID, err)
		}
	}
	multiplier := pricing.PriorityTierMultiplierFromEnv()
	for user, want := range map[string]float64{
		"standard":               list,
		"priority":               list * multiplier,
		"priority-unknown-basis": list * multiplier,
		// A cost the producer stated is kept as stated: the gateway already priced it.
		"priority-stated": 0.5,
		// Usage, not spend: never priced, so never multiplied.
		"priority-subscription": 0,
		"priority-customer":     0,
	} {
		if got := usageCost(t, srv, user); math.Abs(got-want) > 1e-9 {
			t.Errorf("%s cost = %.10f, want %.10f", user, got, want)
		}
	}
}

func TestRecordImageUsageWithNoCataloguePriceRecordsNoImageCost(t *testing.T) {
	srv := NewServer(ledger.NewStore())
	ctx := context.Background()
	image := ImageUsage{Size: "1024x1024", Quality: "high", Count: 2}

	// No tokens, no price: the event is still recorded (usage is never dropped), at zero cost.
	if err := srv.RecordImageUsage(ctx, ledger.Entry{OrgID: "org1", UserID: "img", Model: "gpt-image-1", IdempotencyKey: "img"}, image); err != nil {
		t.Fatalf("RecordImageUsage: %v", err)
	}
	if got := usageCost(t, srv, "img"); got != 0 {
		t.Fatalf("an unpriced image cost %v; it must never be priced at a guess", got)
	}
	entries, err := srv.ledger.ListEntries(ctx, ledger.AggregateFilter{OrgID: "org1", UserID: "img"}, 10)
	if err != nil || len(entries) != 1 {
		t.Fatalf("entries = %+v, err = %v; want the one recorded event", entries, err)
	}

	// Tokens on an image event are priced as RecordUsage prices them; the unknown image adds nothing.
	withTokens := ledger.Entry{OrgID: "org1", UserID: "img-tokens", Model: "gpt-5.6-terra", InputTokens: 1_000, OutputTokens: 100, IdempotencyKey: "img-tokens"}
	if err := srv.RecordImageUsage(ctx, withTokens, image); err != nil {
		t.Fatalf("RecordImageUsage: %v", err)
	}
	if got, want := usageCost(t, srv, "img-tokens"), srv.pricing.CostWithCache("gpt-5.6-terra", 1_000, 100, 0, 0); math.Abs(got-want) > 1e-9 {
		t.Fatalf("token cost on an image event = %v, want %v", got, want)
	}

	// A subscription image event is usage, not spend.
	sub := ledger.Entry{OrgID: "org1", UserID: "img-sub", Model: "gpt-5.6-terra", InputTokens: 1_000, CostBasis: ledger.CostBasisSubscription, IdempotencyKey: "img-sub"}
	if err := srv.RecordImageUsage(ctx, sub, image); err != nil {
		t.Fatalf("RecordImageUsage: %v", err)
	}
	if got := usageCost(t, srv, "img-sub"); got != 0 {
		t.Fatalf("subscription image event priced at %v", got)
	}

	// A stated cost is kept.
	stated := ledger.Entry{OrgID: "org1", UserID: "img-stated", Model: "gpt-image-1", CostUSD: 0.33, IdempotencyKey: "img-stated"}
	if err := srv.RecordImageUsage(ctx, stated, image); err != nil {
		t.Fatalf("RecordImageUsage: %v", err)
	}
	if got := usageCost(t, srv, "img-stated"); got != 0.33 {
		t.Fatalf("stated image cost became %v", got)
	}
}

func TestRecordImageUsageRejectsAnInvalidEntryBeforeWriting(t *testing.T) {
	srv := NewServer(ledger.NewStore())
	if err := srv.RecordImageUsage(context.Background(), ledger.Entry{OrgID: "", UserID: "u"}, ImageUsage{Count: 1}); err == nil {
		t.Fatal("an entry with no org was recorded")
	}
}
