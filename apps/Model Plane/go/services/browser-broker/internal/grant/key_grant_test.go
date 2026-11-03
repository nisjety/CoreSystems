package grant

import (
	"errors"
	"fmt"
	"sync"
	"testing"
	"time"
)

// keyContract is the store-agnostic behaviour of an API-key grant (INNH-10
// MP-2), run against MemoryRepository here and against Routed{PgStore,
// MemoryRepository} in postgres_key_integration_test.go.
var keyContract = []struct {
	name string
	run  func(*testing.T, storeFactory)
}{
	{"Key_GrantCarriesItsKeyIDAndReadsBack", contractKey_GrantCarriesItsKeyIDAndReadsBack},
	{"Key_InvalidKeyIDIsRefused", contractKey_InvalidKeyIDIsRefused},
	{"Key_PerKeyCapCountsOnlyLiveGrantsOfThatKey", contractKey_PerKeyCapCountsOnlyLiveGrantsOfThatKey},
	{"Key_PerKeyCapIsExactUnderConcurrency", contractKey_PerKeyCapIsExactUnderConcurrency},
	{"Key_RevokeByKeyRevokesOnlyThatKeyInEveryOrgAndBothStores", contractKey_RevokeByKeyRevokesOnlyThatKeyInEveryOrgAndBothStores},
	{"Key_GrantIsNeverRenewedDerivedOrAnApprovalParent", contractKey_GrantIsNeverRenewedDerivedOrAnApprovalParent},
}

func TestKeyContract_Memory(t *testing.T) {
	for _, c := range keyContract {
		t.Run(c.name, func(t *testing.T) { c.run(t, memoryStore) })
	}
}

func keyBudget() Budget { return claim(500_000, 20, 300, "request") }

func keySession(t *testing.T, store Repository, keyID string, p CreateSessionParams) *Grant {
	t.Helper()
	p.KeyID = keyID
	if !p.Budget.IsSet() {
		p.Budget = keyBudget()
	}
	if p.TTL == 0 {
		p.TTL = 10 * time.Minute
	}
	return session(t, store, p)
}

func keyRevoker(t *testing.T, store Repository) KeyRevoker {
	t.Helper()
	r, ok := store.(KeyRevoker)
	if !ok {
		t.Fatalf("%T does not implement KeyRevoker", store)
	}
	return r
}

func contractKey_GrantCarriesItsKeyIDAndReadsBack(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	g := keySession(t, store, "key_abc", CreateSessionParams{})
	if g.KeyID != "key_abc" {
		t.Fatalf("issued KeyID = %q", g.KeyID)
	}
	got, err := store.GetScoped(ctx, g.ID, g.OrgID, g.OwnerID)
	if err != nil || got.KeyID != "key_abc" || got.Budget.MicroUSD != 500_000 {
		t.Fatalf("read back: %#v, %v", got, err)
	}
	plain := session(t, store, CreateSessionParams{SessionKey: "plain"})
	got, err = store.GetScoped(ctx, plain.ID, plain.OrgID, plain.OwnerID)
	if err != nil || got.KeyID != "" {
		t.Fatalf("ordinary grant read back with KeyID %q, %v", got.KeyID, err)
	}
	z := keySession(t, store, "key_abc", CreateSessionParams{Zdr: true, SessionKey: "z"})
	got, err = store.GetScoped(ctx, z.ID, z.OrgID, z.OwnerID)
	if err != nil || got.KeyID != "key_abc" || !got.Zdr {
		t.Fatalf("zdr key grant read back: %#v, %v", got, err)
	}
}

func contractKey_InvalidKeyIDIsRefused(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	for _, id := range []string{" key", "key\x00", "kéy", string(make([]byte, 129))} {
		for _, zdr := range []bool{false, true} {
			_, err := store.CreateSession(ctx, CreateSessionParams{
				OrgID: "org-a", OwnerID: "user-a", SessionKey: "s", ScopeURL: "browser://cloud",
				AllowedDomains: []string{"example.com"}, TTL: time.Minute, KeyID: id, Budget: keyBudget(), Zdr: zdr,
			})
			if !errors.Is(err, ErrInvalidKeyID) {
				t.Fatalf("key id %q zdr=%v: error = %v, want ErrInvalidKeyID", id, zdr, err)
			}
		}
		if _, err := keyRevoker(t, store).RevokeByKey(ctx, id, KeyRevokedReason); !errors.Is(err, ErrInvalidKeyID) {
			t.Fatalf("RevokeByKey(%q): %v, want ErrInvalidKeyID", id, err)
		}
	}
}

func contractKey_PerKeyCapCountsOnlyLiveGrantsOfThatKey(t *testing.T, mk storeFactory) {
	store, clock := mk(t)
	a := keySession(t, store, "key_a", CreateSessionParams{SessionKey: "a1", MaxActivePerKey: 2})
	keySession(t, store, "key_a", CreateSessionParams{SessionKey: "a2", MaxActivePerKey: 2})
	_, err := store.CreateSession(ctx, CreateSessionParams{
		OrgID: "org-a", OwnerID: "user-a", SessionKey: "a3", ScopeURL: "browser://cloud",
		AllowedDomains: []string{"example.com"}, TTL: time.Minute, KeyID: "key_a", Budget: keyBudget(), MaxActivePerKey: 2,
	})
	if !errors.Is(err, ErrTooManyGrants) {
		t.Fatalf("third grant of key_a: %v, want ErrTooManyGrants", err)
	}
	// Another key of the same owner, and ordinary grants of the owner, are
	// not counted against key_a.
	keySession(t, store, "key_b", CreateSessionParams{SessionKey: "b1", MaxActivePerKey: 2})
	session(t, store, CreateSessionParams{SessionKey: "plain"})
	// The same key id in another org is a separate count (ids are global in
	// Auth Core; this only proves the count is org-scoped like every query).
	keySession(t, store, "key_a", CreateSessionParams{OrgID: "org-b", SessionKey: "ob", MaxActivePerKey: 2})
	// A revoked grant frees a slot.
	if err := store.RevokeScoped(ctx, a.ID, a.OrgID, a.OwnerID, "done"); err != nil {
		t.Fatal(err)
	}
	keySession(t, store, "key_a", CreateSessionParams{SessionKey: "a4", MaxActivePerKey: 2})
	// An expired grant frees a slot.
	clock.Advance(11 * time.Minute)
	keySession(t, store, "key_a", CreateSessionParams{SessionKey: "a5", MaxActivePerKey: 2})
	keySession(t, store, "key_a", CreateSessionParams{SessionKey: "a6", MaxActivePerKey: 2})
	// Zero means the default (2).
	_, err = store.CreateSession(ctx, CreateSessionParams{
		OrgID: "org-a", OwnerID: "user-a", SessionKey: "a7", ScopeURL: "browser://cloud",
		AllowedDomains: []string{"example.com"}, TTL: time.Minute, KeyID: "key_a", Budget: keyBudget(),
	})
	if !errors.Is(err, ErrTooManyGrants) {
		t.Fatalf("default cap: %v, want ErrTooManyGrants", err)
	}
	// The owner cap applies to key grants too.
	_, err = store.CreateSession(ctx, CreateSessionParams{
		OrgID: "org-a", OwnerID: "user-a", SessionKey: "c1", ScopeURL: "browser://cloud",
		AllowedDomains: []string{"example.com"}, TTL: time.Minute, KeyID: "key_c", Budget: keyBudget(),
		MaxActivePerOwner: 1, MaxActivePerKey: 2,
	})
	if !errors.Is(err, ErrTooManyGrants) {
		t.Fatalf("owner cap with a key grant: %v, want ErrTooManyGrants", err)
	}
}

func contractKey_PerKeyCapIsExactUnderConcurrency(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	const attempts, limit = 16, 2
	var wg sync.WaitGroup
	errs := make([]error, attempts)
	for i := 0; i < attempts; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			_, errs[i] = store.CreateSession(ctx, CreateSessionParams{
				OrgID: "org-a", OwnerID: "user-a", SessionKey: fmt.Sprintf("s%d", i), ScopeURL: "browser://cloud",
				AllowedDomains: []string{"example.com"}, TTL: time.Minute, KeyID: "key_race",
				Budget: keyBudget(), MaxActivePerKey: limit, MaxActivePerOwner: 50,
			})
		}(i)
	}
	wg.Wait()
	ok := 0
	for i, err := range errs {
		switch {
		case err == nil:
			ok++
		case errors.Is(err, ErrTooManyGrants):
		default:
			t.Fatalf("attempt %d: %v", i, err)
		}
	}
	if ok != limit {
		t.Fatalf("%d key grants issued, want exactly %d", ok, limit)
	}
}

func contractKey_RevokeByKeyRevokesOnlyThatKeyInEveryOrgAndBothStores(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	a1 := keySession(t, store, "key_kill", CreateSessionParams{SessionKey: "a1"})
	a2 := keySession(t, store, "key_kill", CreateSessionParams{SessionKey: "a2", Zdr: true})
	other := keySession(t, store, "key_kill", CreateSessionParams{OrgID: "org-b", SessionKey: "b1"})
	keep := keySession(t, store, "key_keep", CreateSessionParams{SessionKey: "k1"})
	plain := session(t, store, CreateSessionParams{SessionKey: "plain"})
	n, err := keyRevoker(t, store).RevokeByKey(ctx, "key_kill", KeyRevokedReason)
	if err != nil || n != 3 {
		t.Fatalf("RevokeByKey = %d, %v; want 3", n, err)
	}
	for _, g := range []*Grant{a1, a2, other} {
		got, err := store.GetScoped(ctx, g.ID, g.OrgID, g.OwnerID)
		if !errors.Is(err, ErrGrantRevoked) {
			t.Fatalf("grant %s after RevokeByKey: %#v, %v; want ErrGrantRevoked", g.ID, got, err)
		}
	}
	for _, g := range []*Grant{keep, plain} {
		if _, err := store.GetScoped(ctx, g.ID, g.OrgID, g.OwnerID); err != nil {
			t.Fatalf("grant %s of another key / no key was touched: %v", g.ID, err)
		}
	}
	// Idempotent: a second sweep revokes nothing and is not an error.
	if n, err := keyRevoker(t, store).RevokeByKey(ctx, "key_kill", KeyRevokedReason); err != nil || n != 0 {
		t.Fatalf("second RevokeByKey = %d, %v; want 0, nil", n, err)
	}
	// A key with no grants is not an error.
	if n, err := keyRevoker(t, store).RevokeByKey(ctx, "key_none", KeyRevokedReason); err != nil || n != 0 {
		t.Fatalf("RevokeByKey(no grants) = %d, %v", n, err)
	}
}

func contractKey_GrantIsNeverRenewedDerivedOrAnApprovalParent(t *testing.T, mk storeFactory) {
	store, _ := mk(t)
	g := keySession(t, store, "key_fixed", CreateSessionParams{})
	if _, err := store.Renew(ctx, RenewParams{OrgID: g.OrgID, OwnerID: g.OwnerID, GrantID: g.ID, TTL: 15 * time.Minute}); !errors.Is(err, ErrRenewNotAllowed) {
		t.Fatalf("renew a key grant (as its owner): %v, want ErrRenewNotAllowed", err)
	}
	if _, err := store.Derive(ctx, deriveParams(g, "run-1", "example.com")); !errors.Is(err, ErrParentNotDerivable) {
		t.Fatalf("derive from a key grant: %v, want ErrParentNotDerivable", err)
	}
	if _, err := store.CreateScopedSensitiveApproval(ctx, approvalOn(g, []string{"example.com"}, time.Minute)); !errors.Is(err, ErrApprovalParentInvalid) {
		t.Fatalf("approval under a key grant: %v, want ErrApprovalParentInvalid", err)
	}
	got, err := store.GetScoped(ctx, g.ID, g.OrgID, g.OwnerID)
	if err != nil || !got.ExpiresAt.Equal(g.ExpiresAt) {
		t.Fatalf("key grant changed: %#v, %v", got, err)
	}
}
