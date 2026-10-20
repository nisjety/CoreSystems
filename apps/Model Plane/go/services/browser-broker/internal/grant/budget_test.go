package grant

import (
	"errors"
	"testing"
	"time"
)

// budgetContract is the store-agnostic budget-claim behaviour of WS6.E, run
// against MemoryRepository here and against Routed{PgStore, MemoryRepository}
// in postgres_integration_test.go.
var budgetContract = []struct {
	name string
	run  func(*testing.T, storeFactory)
}{
	{"Budget_ASessionClaimIsStoredAndReturnedVerbatim", contractBudget_ASessionClaimIsStoredAndReturnedVerbatim},
	{"Budget_NoClaimReadsBackAsTheZeroBudget", contractBudget_NoClaimReadsBackAsTheZeroBudget},
	{"Budget_MalformedClaimsAreRefused", contractBudget_MalformedClaimsAreRefused},
	{"Budget_ADerivedGrantNeverWidensItsParent", contractBudget_ADerivedGrantNeverWidensItsParent},
	{"Budget_ADeriveWithoutAClaimInheritsTheParents", contractBudget_ADeriveWithoutAClaimInheritsTheParents},
	{"Budget_ADeriveMayTightenOneDimensionAndKeepTheRest", contractBudget_ADeriveMayTightenOneDimensionAndKeepTheRest},
	{"Budget_AParentWithoutAClaimLetsTheRunSetOne", contractBudget_AParentWithoutAClaimLetsTheRunSetOne},
	{"Budget_AReplayKeepsTheIssuedClaimAndADifferentClaimConflicts", contractBudget_AReplayKeepsTheIssuedClaimAndADifferentClaimConflicts},
	{"Budget_AZdrGrantCarriesAClaimInMemoryOnly", contractBudget_AZdrGrantCarriesAClaimInMemoryOnly},
}

func TestBudgetContract_Memory(t *testing.T) {
	for _, c := range budgetContract {
		t.Run(c.name, func(t *testing.T) { c.run(t, memoryStore) })
	}
}

func claim(microUSD int64, steps, runtimeS int32, source string) Budget {
	return Budget{MicroUSD: microUSD, MaxSteps: steps, MaxRuntimeS: runtimeS, Source: source}
}

func withBudget(parent *Grant, key string, b Budget, domains ...string) DeriveParams {
	p := deriveParams(parent, key, domains...)
	p.Budget = b
	return p
}

func contractBudget_ASessionClaimIsStoredAndReturnedVerbatim(t *testing.T, mk storeFactory) {
	store, clock := mk(t)
	g := session(t, store, CreateSessionParams{
		TTL:    15 * time.Minute,
		Budget: claim(2_500_000, 40, 600, BudgetSourceOrgDefault),
	})
	want := Budget{
		MicroUSD: 2_500_000, MaxSteps: 40, MaxRuntimeS: 600,
		Source: BudgetSourceOrgDefault, MintedAtUnix: clock.Now().Unix(),
	}
	if g.Budget != want {
		t.Fatalf("created claim = %#v, want %#v", g.Budget, want)
	}
	got, err := store.GetScoped(ctx, g.ID, g.OrgID, g.OwnerID)
	if err != nil {
		t.Fatal(err)
	}
	if got.Budget != want {
		t.Fatalf("stored claim = %#v, want %#v", got.Budget, want)
	}
	// A caller cannot mutate a stored grant through the copy it was handed.
	g.Budget.MicroUSD = 1
	again, _ := store.GetScoped(ctx, g.ID, g.OrgID, g.OwnerID)
	if again.Budget != want {
		t.Fatalf("claim changed through a returned copy: %#v", again.Budget)
	}
}

func contractBudget_NoClaimReadsBackAsTheZeroBudget(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	g := session(t, store, CreateSessionParams{TTL: 15 * time.Minute})
	got, err := store.GetScoped(ctx, g.ID, g.OrgID, g.OwnerID)
	if err != nil {
		t.Fatal(err)
	}
	if got.Budget != (Budget{}) || got.Budget.IsSet() {
		t.Fatalf("a grant minted without a claim carries %#v", got.Budget)
	}
	run, err := store.Derive(ctx, deriveParams(g, "k"))
	if err != nil {
		t.Fatal(err)
	}
	if run.Budget != (Budget{}) {
		t.Fatalf("a derive from an unclaimed parent with no request carries %#v", run.Budget)
	}
}

func contractBudget_MalformedClaimsAreRefused(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	parent := session(t, store, CreateSessionParams{TTL: 15 * time.Minute})
	for name, bad := range map[string]Budget{
		"negative cost":           claim(-1, 0, 0, BudgetSourceRequest),
		"negative steps":          claim(0, -1, 0, BudgetSourceRequest),
		"negative runtime":        claim(0, 0, -5, BudgetSourceRequest),
		"unknown source":          claim(1, 0, 0, "guess"),
		"bound without a source":  claim(1_000_000, 10, 0, ""),
		"source with no bound":    {Source: BudgetSourceRequest},
		"a caller-supplied stamp": {MicroUSD: 1, Source: BudgetSourceRequest, MintedAtUnix: 1_700_000_000},
	} {
		t.Run(name, func(t *testing.T) {
			if _, err := store.CreateSession(ctx, CreateSessionParams{
				OrgID: "org-a", OwnerID: "user-a", SessionKey: "s-" + name, ScopeURL: "browser://cloud",
				AllowedDomains: []string{"example.com"}, TTL: time.Minute, Budget: bad,
			}); !errors.Is(err, ErrInvalidBudget) {
				t.Fatalf("CreateSession: error = %v, want ErrInvalidBudget", err)
			}
			if _, err := store.Derive(ctx, withBudget(parent, "d-"+name, bad)); !errors.Is(err, ErrInvalidBudget) {
				t.Fatalf("Derive: error = %v, want ErrInvalidBudget", err)
			}
		})
	}
	// Nothing was issued by the refused derives.
	if run, err := store.Derive(ctx, deriveParams(parent, "after-refusals")); err != nil || run.Budget.IsSet() {
		t.Fatalf("a clean derive after the refusals: %#v, %v", run, err)
	}
}

func contractBudget_ADerivedGrantNeverWidensItsParent(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	parent := session(t, store, CreateSessionParams{
		TTL: 15 * time.Minute, Budget: claim(5_000_000, 20, 600, BudgetSourceOrgDefault),
	})
	for name, wider := range map[string]Budget{
		"cost above the parent":         claim(5_000_001, 0, 0, BudgetSourceRequest),
		"steps above the parent":        claim(0, 21, 0, BudgetSourceRequest),
		"runtime above the parent":      claim(0, 0, 601, BudgetSourceRequest),
		"one dimension tight, one wide": claim(1_000_000, 21, 0, BudgetSourceRequest),
	} {
		t.Run(name, func(t *testing.T) {
			if _, err := store.Derive(ctx, withBudget(parent, "w-"+name, wider)); !errors.Is(err, ErrDeriveWidensBudget) {
				t.Fatalf("error = %v, want ErrDeriveWidensBudget", err)
			}
		})
	}
	// Equal to the parent is not wider.
	run, err := store.Derive(ctx, withBudget(parent, "equal", claim(5_000_000, 20, 600, BudgetSourceRequest)))
	if err != nil {
		t.Fatalf("a claim equal to the parent's: %v", err)
	}
	if run.Budget.MicroUSD != 5_000_000 || run.Budget.MaxSteps != 20 || run.Budget.MaxRuntimeS != 600 {
		t.Fatalf("equal claim = %#v", run.Budget)
	}
	// Nothing about the failed attempts was kept: a refused key is free.
	if _, err := store.Derive(ctx, withBudget(parent, "w-cost above the parent", claim(4_000_000, 0, 0, BudgetSourceRequest))); err != nil {
		t.Fatalf("a refused key can be reused with a tighter claim: %v", err)
	}
}

func contractBudget_ADeriveWithoutAClaimInheritsTheParents(t *testing.T, mk storeFactory) {
	store, clock := mk(t)
	parent := session(t, store, CreateSessionParams{
		TTL: 15 * time.Minute, Budget: claim(5_000_000, 20, 600, BudgetSourceEntitlement),
	})
	clock.Advance(time.Minute)
	run, err := store.Derive(ctx, deriveParams(parent, "inherit"))
	if err != nil {
		t.Fatal(err)
	}
	want := parent.Budget // the label and the stamp are the parent's, not re-minted
	if run.Budget != want {
		t.Fatalf("inherited claim = %#v, want %#v", run.Budget, want)
	}
	stored, err := store.GetScoped(ctx, run.ID, run.OrgID, run.OwnerID)
	if err != nil || stored.Budget != want {
		t.Fatalf("stored inherited claim = %#v, %v", stored, err)
	}
}

func contractBudget_ADeriveMayTightenOneDimensionAndKeepTheRest(t *testing.T, mk storeFactory) {
	store, clock := mk(t)
	parent := session(t, store, CreateSessionParams{
		TTL: 15 * time.Minute, Budget: claim(5_000_000, 20, 600, BudgetSourceOrgDefault),
	})
	clock.Advance(2 * time.Minute)
	run, err := store.Derive(ctx, withBudget(parent, "tight", claim(1_000_000, 0, 0, BudgetSourceRequest)))
	if err != nil {
		t.Fatal(err)
	}
	want := Budget{
		MicroUSD: 1_000_000, MaxSteps: 20, MaxRuntimeS: 600,
		Source: BudgetSourceRequest, MintedAtUnix: clock.Now().Unix(),
	}
	if run.Budget != want {
		t.Fatalf("tightened claim = %#v, want %#v", run.Budget, want)
	}
	// The parent is untouched by its child.
	got, _ := store.GetScoped(ctx, parent.ID, parent.OrgID, parent.OwnerID)
	if got.Budget != parent.Budget {
		t.Fatalf("parent claim changed to %#v", got.Budget)
	}
}

func contractBudget_AParentWithoutAClaimLetsTheRunSetOne(t *testing.T, mk storeFactory) {
	store, clock := mk(t)
	parent := session(t, store, CreateSessionParams{TTL: 15 * time.Minute})
	run, err := store.Derive(ctx, withBudget(parent, "set", claim(0, 12, 0, BudgetSourceRequest)))
	if err != nil {
		t.Fatal(err)
	}
	want := Budget{MaxSteps: 12, Source: BudgetSourceRequest, MintedAtUnix: clock.Now().Unix()}
	if run.Budget != want {
		t.Fatalf("claim = %#v, want %#v", run.Budget, want)
	}
}

func contractBudget_AReplayKeepsTheIssuedClaimAndADifferentClaimConflicts(t *testing.T, mk storeFactory) {
	store, clock := mk(t)
	parent := session(t, store, CreateSessionParams{TTL: 15 * time.Minute})
	first, err := store.Derive(ctx, withBudget(parent, "replay", claim(1_000_000, 10, 0, BudgetSourceRequest)))
	if err != nil {
		t.Fatal(err)
	}
	clock.Advance(time.Minute)
	again, err := store.Derive(ctx, withBudget(parent, "replay", claim(1_000_000, 10, 0, BudgetSourceRequest)))
	if err != nil {
		t.Fatalf("an identical replay: %v", err)
	}
	if again.ID != first.ID || again.Budget != first.Budget {
		t.Fatalf("replay = %#v, first = %#v: a replay must not re-stamp or re-mint", again, first)
	}
	for name, other := range map[string]Budget{
		"looser":   claim(2_000_000, 10, 0, BudgetSourceRequest),
		"tighter":  claim(500_000, 10, 0, BudgetSourceRequest),
		"no claim": {},
		"label":    claim(1_000_000, 10, 0, BudgetSourceOrgDefault),
	} {
		if _, err := store.Derive(ctx, withBudget(parent, "replay", other)); !errors.Is(err, ErrDeriveConflict) {
			t.Fatalf("%s replay: error = %v, want ErrDeriveConflict", name, err)
		}
	}
	// The conflicts did not change the issued grant.
	got, err := store.GetScoped(ctx, first.ID, first.OrgID, first.OwnerID)
	if err != nil || got.Budget != first.Budget {
		t.Fatalf("issued grant after conflicts = %#v, %v", got, err)
	}
}

func contractBudget_AZdrGrantCarriesAClaimInMemoryOnly(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	parent := session(t, store, CreateSessionParams{
		TTL: 15 * time.Minute, Zdr: true, Budget: claim(3_000_000, 15, 300, BudgetSourceRequest),
	})
	if !parent.Zdr || parent.ID[:2] != zdrIDPrefix {
		t.Fatalf("not a ZDR grant: %#v", parent)
	}
	got, err := store.GetScoped(ctx, parent.ID, parent.OrgID, parent.OwnerID)
	if err != nil || got.Budget.MicroUSD != 3_000_000 || got.Budget.MaxSteps != 15 || got.Budget.MaxRuntimeS != 300 {
		t.Fatalf("ZDR claim read back = %#v, %v", got, err)
	}
	run, err := store.Derive(ctx, deriveParams(parent, "zdr-run"))
	if err != nil || !run.Zdr || run.Budget.MicroUSD != 3_000_000 {
		t.Fatalf("ZDR run grant = %#v, %v", run, err)
	}
}
