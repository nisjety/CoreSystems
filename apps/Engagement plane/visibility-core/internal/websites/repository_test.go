package websites

import (
	"errors"
	"os"
	"path/filepath"
	"sync"
	"testing"
)

func privateDir(t *testing.T) string {
	t.Helper()
	dir := t.TempDir()
	if err := os.Chmod(dir, 0700); err != nil {
		t.Fatal(err)
	}
	return dir
}
func TestRepositoryDurabilityOrganizationAndInvalidInput(t *testing.T) {
	path := filepath.Join(privateDir(t), "websites.json")
	repo, err := Open(path, "org-one")
	if err != nil {
		t.Fatal(err)
	}
	err = repo.Update(func(records []Record) ([]Record, error) {
		return []Record{{Website: newWebsite("https://example.com/", "2026-10-21T12:00:00Z")}}, nil
	})
	if err != nil {
		t.Fatal(err)
	}
	reloaded, err := Open(path, "org-one")
	if err != nil || len(reloaded.List()) != 1 {
		t.Fatalf("durability: %v", err)
	}
	if _, err := Open(path, "org-two"); err == nil {
		t.Fatal("accepted another organization's registry")
	}
	info, _ := os.Stat(path)
	if info.Mode().Perm() != 0600 {
		t.Fatal("unsafe registry permissions")
	}
	first := reloaded.List()
	first[0].Website.Site = "https://other.example/"
	if reloaded.List()[0].Website.Site != "https://example.com/" {
		t.Fatal("repository leaked mutable state")
	}
	if err := os.WriteFile(path, []byte(`{"version":1,"organizationId":"org-one","records":`), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := Open(path, "org-one"); err == nil {
		t.Fatal("corruption accepted")
	}
}

func TestRepositoryRejectsSymlinksAndPreservesFailedTransaction(t *testing.T) {
	dir := privateDir(t)
	path := filepath.Join(dir, "websites.json")
	repo, err := Open(path, "org-one")
	if err != nil {
		t.Fatal(err)
	}
	wantErr := errors.New("rejected")
	if err := repo.Update(func([]Record) ([]Record, error) { return nil, wantErr }); !errors.Is(err, wantErr) {
		t.Fatal(err)
	}
	if len(repo.List()) != 0 {
		t.Fatal("failed transaction changed records")
	}
	link := filepath.Join(dir, "link.json")
	if err := os.Symlink(path, link); err != nil {
		t.Fatal(err)
	}
	if _, err := Open(link, "org-one"); err == nil {
		t.Fatal("symlink accepted")
	}
}

func TestRepositoryConcurrentUpdatesAndDuplicateRejection(t *testing.T) {
	repo, err := Open(filepath.Join(privateDir(t), "websites.json"), "org-one")
	if err != nil {
		t.Fatal(err)
	}
	var wg sync.WaitGroup
	for range 12 {
		wg.Add(1)
		go func() {
			defer wg.Done()
			_ = repo.Update(func(records []Record) ([]Record, error) {
				if len(records) > 0 {
					return records, nil
				}
				return []Record{{Website: newWebsite("https://example.com/", "2026-10-21T12:00:00Z")}}, nil
			})
		}()
	}
	wg.Wait()
	if len(repo.List()) != 1 {
		t.Fatal("concurrent update lost atomicity")
	}
	if err := repo.Update(func(records []Record) ([]Record, error) { return append(records, records[0]), nil }); err == nil {
		t.Fatal("duplicate accepted")
	}
}
