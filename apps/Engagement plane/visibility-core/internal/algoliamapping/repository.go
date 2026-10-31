package algoliamapping

import (
	"encoding/json"
	"errors"
	"fmt"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"io"
	"os"
	"path/filepath"
	"time"
)

var errCommitted = errors.New("Algolia mapping committed but directory sync failed")

type repository struct {
	path  string
	state diskState
}

func privateDir(path string) bool {
	i, e := os.Lstat(path)
	return e == nil && i.IsDir() && i.Mode().Perm() == 0700
}
func open(path, org string) (*repository, error) {
	if !filepath.IsAbs(path) || filepath.Clean(path) != path || !orgPattern.MatchString(org) || !privateDir(filepath.Dir(path)) {
		return nil, ErrInvalid
	}
	r := &repository{path: path, state: diskState{Version: 1, OrganizationID: org, Entries: []entry{}}}
	info, err := os.Lstat(path)
	if os.IsNotExist(err) {
		if err = r.persist(r.state); err != nil {
			return nil, err
		}
		return r, nil
	}
	if err != nil || !info.Mode().IsRegular() || info.Mode().Perm() != 0600 || info.Size() > maxBytes {
		return nil, ErrInvalid
	}
	f, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	actual, err := f.Stat()
	if err != nil || !os.SameFile(info, actual) {
		return nil, ErrInvalid
	}
	raw, err := io.ReadAll(io.LimitReader(f, maxBytes+1))
	if err != nil || len(raw) > maxBytes || !unique(raw) {
		return nil, ErrInvalid
	}
	m, ok := object(raw, []string{"version", "organizationId", "entries"}, nil)
	if !ok {
		return nil, ErrInvalid
	}
	var entries []json.RawMessage
	if json.Unmarshal(m["entries"], &entries) != nil || entries == nil {
		return nil, ErrInvalid
	}
	for _, rawEntry := range entries {
		e, ok := object(rawEntry, []string{"mapping", "version"}, []string{"snapshotFingerprint", "snapshotGeneration", "snapshotObservedAt", "snapshotDigest"})
		if !ok {
			return nil, ErrInvalid
		}
		if _, ok = object(e["mapping"], []string{"websiteId", "site", "websiteRevision", "status"}, []string{"connectionId", "applicationId", "indexName"}); !ok {
			return nil, ErrInvalid
		}
	}
	if json.Unmarshal(raw, &r.state) != nil || !validState(r.state, org) {
		return nil, ErrInvalid
	}
	return r, nil
}
func validState(s diskState, org string) bool {
	if s.Version != 1 || s.OrganizationID != org || s.Entries == nil || len(s.Entries) > maxEntries {
		return false
	}
	seen := map[string]bool{}
	for _, e := range s.Entries {
		m := e.Mapping
		site, err := pulse.CanonicalSite(m.Site)
		if err != nil || site != m.Site || !idPattern.MatchString(m.WebsiteID) || seen[m.WebsiteID] || m.WebsiteRevision < 1 || m.WebsiteRevision > 9007199254740991 || !fingerprintPattern.MatchString(e.Version) {
			return false
		}
		seen[m.WebsiteID] = true
		if m.Status == "mapped" {
			if !connectionPattern.MatchString(m.ConnectionID) || !appPattern.MatchString(m.ApplicationID) || !indexPattern.MatchString(m.IndexName) {
				return false
			}
		} else if m.Status != "not_mapped" || m.ConnectionID != "" || m.ApplicationID != "" || m.IndexName != "" {
			return false
		}
		if e.SnapshotObservedAt != "" {
			if m.Status != "mapped" || e.SnapshotFingerprint != fingerprint(e) || !generationPattern.MatchString(e.SnapshotGeneration) || !fingerprintPattern.MatchString(e.SnapshotDigest) {
				return false
			}
			if _, err = time.Parse(time.RFC3339Nano, e.SnapshotObservedAt); err != nil {
				return false
			}
		} else if e.SnapshotFingerprint != "" || e.SnapshotGeneration != "" || e.SnapshotDigest != "" {
			return false
		}
	}
	return true
}
func (r *repository) save(e entry) error {
	entries := make([]entry, 0, len(r.state.Entries)+1)
	for _, prior := range r.state.Entries {
		if prior.Mapping.WebsiteID != e.Mapping.WebsiteID {
			entries = append(entries, prior)
		}
	}
	entries = append(entries, e)
	next := diskState{Version: 1, OrganizationID: r.state.OrganizationID, Entries: entries}
	if !validState(next, r.state.OrganizationID) {
		return ErrInvalid
	}
	err := r.persist(next)
	if err == nil || errors.Is(err, errCommitted) {
		r.state = next
	}
	return err
}
func (r *repository) persist(s diskState) error {
	if !privateDir(filepath.Dir(r.path)) {
		return ErrInvalid
	}
	if i, e := os.Lstat(r.path); e == nil {
		if !i.Mode().IsRegular() || i.Mode().Perm() != 0600 {
			return ErrInvalid
		}
	} else if !os.IsNotExist(e) {
		return e
	}
	raw, err := json.Marshal(s)
	if err != nil || len(raw) > maxBytes {
		return ErrInvalid
	}
	f, err := os.CreateTemp(filepath.Dir(r.path), ".algolia-mappings-*")
	if err != nil {
		return err
	}
	defer os.Remove(f.Name())
	if err = f.Chmod(0600); err == nil {
		_, err = f.Write(raw)
	}
	if err == nil {
		err = f.Sync()
	}
	closed := f.Close()
	if err == nil {
		err = closed
	}
	if err != nil {
		return err
	}
	if err = os.Rename(f.Name(), r.path); err != nil {
		return err
	}
	dir, err := os.Open(filepath.Dir(r.path))
	if err != nil {
		return fmt.Errorf("%w: %v", errCommitted, err)
	}
	defer dir.Close()
	if err = dir.Sync(); err != nil {
		return fmt.Errorf("%w: %v", errCommitted, err)
	}
	return nil
}
