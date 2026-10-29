package sitesources

import (
	"bytes"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
)

const maxCollectionBytes = 128 << 10

type collectionRepository struct {
	path  string
	state collectionDisk
}

func openCollections(path, org string) (*collectionRepository, error) {
	if !filepath.IsAbs(path) || filepath.Clean(path) != path || !orgPattern.MatchString(org) || !privateDir(filepath.Dir(path)) {
		return nil, ErrInvalid
	}
	r := &collectionRepository{path: path, state: collectionDisk{1, org, []collectionJob{}}}
	info, err := os.Lstat(path)
	if os.IsNotExist(err) {
		if err = r.save(r.state.Entries); err != nil {
			return nil, err
		}
		return r, nil
	}
	if err != nil || !info.Mode().IsRegular() || info.Mode().Perm() != 0600 || info.Size() > maxCollectionBytes {
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
	raw, err := io.ReadAll(io.LimitReader(f, maxCollectionBytes+1))
	if err != nil || len(raw) > maxCollectionBytes || !uniqueJSON(raw) {
		return nil, ErrInvalid
	}
	d := json.NewDecoder(bytes.NewReader(raw))
	d.DisallowUnknownFields()
	if d.Decode(&r.state) != nil || d.Decode(new(any)) != io.EOF || !validCollectionDisk(r.state, org) {
		return nil, ErrInvalid
	}
	return r, nil
}
func validCollectionDisk(d collectionDisk, org string) bool {
	if d.Version != 1 || d.OrganizationID != org || d.Entries == nil || len(d.Entries) > 64 {
		return false
	}
	seen := map[string]bool{}
	for _, j := range d.Entries {
		canonical, err := pulse.CanonicalSite(j.Site)
		if err != nil || canonical != j.Site || !idPattern.MatchString(j.WebsiteID) || j.WebsiteRevision < 1 || j.WebsiteRevision > 9007199254740991 || j.Provider != "algolia" || !fingerprintPattern.MatchString(j.Fingerprint) || !fingerprintPattern.MatchString(j.RunID) || j.Attempts < 0 || j.Attempts > 1000 || seen[j.WebsiteID] {
			return false
		}
		seen[j.WebsiteID] = true
		switch j.Status {
		case "queued", "syncing", "ready", "needs_attention", "reconnect_required":
		default:
			return false
		}
		switch j.Reason {
		case "", "partial_report", "provider_unavailable", "not_configured", "reconnect_required", "context_changed":
		default:
			return false
		}
		for _, v := range []string{j.NextRunAt, j.LastAttemptAt, j.LastSuccessAt} {
			if v != "" && collectionTime(v).IsZero() {
				return false
			}
		}
	}
	return true
}
func (r *collectionRepository) save(entries []collectionJob) error {
	next := collectionDisk{1, r.state.OrganizationID, append([]collectionJob{}, entries...)}
	if !validCollectionDisk(next, r.state.OrganizationID) || !privateDir(filepath.Dir(r.path)) {
		return ErrInvalid
	}
	if info, err := os.Lstat(r.path); err == nil {
		if !info.Mode().IsRegular() || info.Mode().Perm() != 0600 {
			return ErrInvalid
		}
	} else if !os.IsNotExist(err) {
		return err
	}
	raw, err := json.Marshal(next)
	if err != nil || len(raw) > maxCollectionBytes {
		return ErrCapacity
	}
	f, err := os.CreateTemp(filepath.Dir(r.path), ".source-jobs-*")
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
	if err == nil {
		err = dir.Sync()
		_ = dir.Close()
	}
	r.state = next // Rename is committed even if the following directory sync failed.
	if err != nil {
		return errors.Join(errCommitted, err)
	}
	return nil
}
