package mailtask

import (
	"context"
	"errors"
	"sync"
	"testing"
	"time"
)

// fakeRepository is an in-memory Repository so Service's alias-lifecycle
// logic is testable without a live Postgres instance (mirrors this codebase's
// convention of gating real-DB coverage behind TEST_DATABASE_URL and keeping
// pure logic tests independent of it).
type fakeRepository struct {
	mu   sync.Mutex
	rows map[string]Address // keyed by alias
}

func newFakeRepository() *fakeRepository {
	return &fakeRepository{rows: make(map[string]Address)}
}

func (f *fakeRepository) ActiveForUser(_ context.Context, orgID, userID string) (*Address, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	for _, row := range f.rows {
		if row.OrgID == orgID && row.UserID == userID && row.IsLive() {
			copied := row
			return &copied, nil
		}
	}
	return nil, nil
}

func (f *fakeRepository) Insert(_ context.Context, address Address) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	if _, exists := f.rows[address.Alias]; exists {
		return ErrAliasCollision
	}
	f.rows[address.Alias] = address
	return nil
}

func (f *fakeRepository) RevokeActive(_ context.Context, orgID, userID string, revokedAt time.Time) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	for alias, row := range f.rows {
		if row.OrgID == orgID && row.UserID == userID && row.IsLive() {
			row.RevokedAt = &revokedAt
			f.rows[alias] = row
		}
	}
	return nil
}

func (f *fakeRepository) ByAlias(_ context.Context, alias string) (*Address, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	row, ok := f.rows[alias]
	if !ok {
		return nil, nil
	}
	copied := row
	return &copied, nil
}

func TestGenerateAliasIsIdempotentPerOrgUser(t *testing.T) {
	svc := NewService(newFakeRepository())
	ctx := context.Background()

	first, err := svc.GenerateAlias(ctx, "org-1", "user-1")
	if err != nil {
		t.Fatalf("GenerateAlias() first call error = %v", err)
	}
	if first.Alias == "" {
		t.Fatal("GenerateAlias() returned an empty alias")
	}

	second, err := svc.GenerateAlias(ctx, "org-1", "user-1")
	if err != nil {
		t.Fatalf("GenerateAlias() second call error = %v", err)
	}
	if second.Alias != first.Alias {
		t.Fatalf("GenerateAlias() is not idempotent: got %q then %q", first.Alias, second.Alias)
	}

	// A different user in the same org must get its own alias.
	other, err := svc.GenerateAlias(ctx, "org-1", "user-2")
	if err != nil {
		t.Fatalf("GenerateAlias() for a different user error = %v", err)
	}
	if other.Alias == first.Alias {
		t.Fatal("GenerateAlias() returned the same alias for two different users")
	}
}

func TestGenerateAliasRejectsMissingIdentifiers(t *testing.T) {
	svc := NewService(newFakeRepository())
	ctx := context.Background()

	if _, err := svc.GenerateAlias(ctx, "", "user-1"); err == nil {
		t.Fatal("GenerateAlias() with empty org ID should error")
	}
	if _, err := svc.GenerateAlias(ctx, "org-1", "  "); err == nil {
		t.Fatal("GenerateAlias() with blank user ID should error")
	}
}

func TestRevokeAliasInvalidatesTheOldAliasImmediatelyAndMintsAReplacement(t *testing.T) {
	svc := NewService(newFakeRepository())
	ctx := context.Background()

	original, err := svc.GenerateAlias(ctx, "org-1", "user-1")
	if err != nil {
		t.Fatalf("GenerateAlias() error = %v", err)
	}

	if err := svc.RevokeAlias(ctx, "org-1", "user-1"); err != nil {
		t.Fatalf("RevokeAlias() error = %v", err)
	}

	// The old alias must stop resolving right away.
	if _, err := svc.ResolveAlias(ctx, original.Alias); !errors.Is(err, ErrRevoked) {
		t.Fatalf("ResolveAlias(old alias) after revoke = %v, want ErrRevoked", err)
	}

	// A fresh alias must already be live by the time the next GET runs (the
	// SPA's Settings "Regenerate" flow is DELETE then GET).
	replacement, err := svc.GenerateAlias(ctx, "org-1", "user-1")
	if err != nil {
		t.Fatalf("GenerateAlias() after revoke error = %v", err)
	}
	if replacement.Alias == original.Alias {
		t.Fatal("RevokeAlias() did not mint a genuinely new alias")
	}
	if _, err := svc.ResolveAlias(ctx, replacement.Alias); err != nil {
		t.Fatalf("ResolveAlias(new alias) = %v, want it to resolve", err)
	}
}

func TestResolveAliasDistinguishesNotFoundFromRevoked(t *testing.T) {
	svc := NewService(newFakeRepository())
	ctx := context.Background()

	// Never existed.
	if _, err := svc.ResolveAlias(ctx, "does-not-exist"); !errors.Is(err, ErrNotFound) {
		t.Fatalf("ResolveAlias(unknown) = %v, want ErrNotFound", err)
	}

	// Existed, then was revoked.
	address, err := svc.GenerateAlias(ctx, "org-1", "user-1")
	if err != nil {
		t.Fatalf("GenerateAlias() error = %v", err)
	}
	if err := svc.RevokeAlias(ctx, "org-1", "user-1"); err != nil {
		t.Fatalf("RevokeAlias() error = %v", err)
	}
	_, err = svc.ResolveAlias(ctx, address.Alias)
	if !errors.Is(err, ErrRevoked) {
		t.Fatalf("ResolveAlias(revoked) = %v, want ErrRevoked (must not collapse to ErrNotFound at this layer)", err)
	}
	if errors.Is(err, ErrNotFound) {
		t.Fatal("ErrRevoked must not also satisfy errors.Is(ErrNotFound) -- callers below the HTTP layer need the real distinction")
	}
}

func TestResolveAliasNormalizesCaseAndWhitespace(t *testing.T) {
	svc := NewService(newFakeRepository())
	ctx := context.Background()

	address, err := svc.GenerateAlias(ctx, "org-1", "user-1")
	if err != nil {
		t.Fatalf("GenerateAlias() error = %v", err)
	}

	resolved, err := svc.ResolveAlias(ctx, "  "+address.Alias+"  ")
	if err != nil {
		t.Fatalf("ResolveAlias() with surrounding whitespace error = %v", err)
	}
	if resolved.OrgID != "org-1" || resolved.UserID != "user-1" {
		t.Fatalf("ResolveAlias() returned %+v", resolved)
	}
}
