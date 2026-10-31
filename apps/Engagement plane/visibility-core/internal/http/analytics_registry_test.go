package http

import (
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"os"
	"path/filepath"
	"testing"
)

func newAnalyticsRegistry(t *testing.T) *websites.Service {
	t.Helper()
	dir := t.TempDir()
	if err := os.Chmod(dir, 0700); err != nil {
		t.Fatal(err)
	}
	repo, err := websites.Open(filepath.Join(dir, "sites.json"), "org-one")
	if err != nil {
		t.Fatal(err)
	}
	return websites.NewService(repo, nil, nil)
}
