package sharepoint

import (
	"context"
	"testing"
)

func TestDisconnectedBrowserReturnsNotConfigured(t *testing.T) {
	t.Parallel()

	browser := DisconnectedBrowser{}

	if _, err := browser.ListSites(context.Background(), "org-123"); err != ErrNotConfigured {
		t.Fatalf("ListSites err = %v, want %v", err, ErrNotConfigured)
	}

	if _, err := browser.ListItems(context.Background(), "org-123", "site-1", "/"); err != ErrNotConfigured {
		t.Fatalf("ListItems err = %v, want %v", err, ErrNotConfigured)
	}
}
