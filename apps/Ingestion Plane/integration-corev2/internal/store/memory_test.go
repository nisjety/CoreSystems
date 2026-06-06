package store

import (
	"context"
	"testing"
	"time"
)

func TestMemoryRepositoryStoresAndDeletesConnections(t *testing.T) {
	repo := NewMemoryRepository()
	ctx := context.Background()
	connection := Connection{
		ID:                   "conn-1",
		ProviderKey:          "microsoft",
		ConnectorType:        "microsoft-graph",
		OrganizationID:       "org-1",
		Status:               "active",
		AccessTokenExpiresAt: time.Now().Add(time.Hour),
	}
	if _, err := repo.UpsertConnection(ctx, connection); err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	got, err := repo.FindActiveConnection(ctx, "org-1", "microsoft-graph")
	if err != nil {
		t.Fatalf("FindActiveConnection error: %v", err)
	}
	if got.ID != "conn-1" {
		t.Fatalf("connection ID = %q, want conn-1", got.ID)
	}
	if _, err := repo.MarkConnectionDeleted(ctx, "conn-1"); err != nil {
		t.Fatalf("MarkConnectionDeleted error: %v", err)
	}
	if _, err := repo.FindActiveConnection(ctx, "org-1", "microsoft-graph"); err != ErrNotFound {
		t.Fatalf("FindActiveConnection error = %v, want ErrNotFound", err)
	}
}
