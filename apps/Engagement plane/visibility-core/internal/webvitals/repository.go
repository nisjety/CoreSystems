package webvitals

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"io"
	"os"
	"path/filepath"
	"time"
)

type repository struct {
	path  string
	state diskState
}

var errCommitted = errors.New("web vitals committed but directory sync failed")

func privateDir(path string) bool {
	i, e := os.Lstat(path)
	return e == nil && i.IsDir() && i.Mode().Perm() == 0700
}
func open(path, org string) (*repository, error) {
	if !filepath.IsAbs(path) || filepath.Clean(path) != path || !orgPattern.MatchString(org) || !privateDir(filepath.Dir(path)) {
		return nil, ErrInvalid
	}
	r := &repository{path: path, state: diskState{Version: 1, OrganizationID: org, Entries: []Export{}}}
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
	if err != nil || len(raw) > maxBytes {
		return nil, ErrInvalid
	}
	dec := json.NewDecoder(bytes.NewReader(raw))
	dec.DisallowUnknownFields()
	var state diskState
	if !uniqueJSON(raw) || dec.Decode(&state) != nil || dec.Decode(new(any)) != io.EOF || !validState(state, org) {
		return nil, ErrInvalid
	}
	r.state = state
	return r, nil
}
func validState(state diskState, org string) bool {
	if state.Version != 1 || state.OrganizationID != org || state.Entries == nil || len(state.Entries) > maxEntries {
		return false
	}
	keys := map[string]bool{}
	count := 0
	for _, e := range state.Entries {
		key := e.WebsiteID + ":" + e.Environment
		if keys[key] || !validExport(e) {
			return false
		}
		keys[key] = true
		observed, _ := time.Parse(time.RFC3339Nano, e.ObservedAt)
		deduped, err := normalize(e, websites.Website{ID: e.WebsiteID, Site: e.Site, Revision: e.WebsiteRevision}, e.Environment, observed)
		if err != nil || len(deduped.Samples) != len(e.Samples) {
			return false
		}
		count += len(e.Samples)
	}
	return count <= maxSamples
}
func (r *repository) save(entry Export, now time.Time) error {
	entries := make([]Export, 0, len(r.state.Entries)+1)
	for _, e := range r.state.Entries {
		if e.WebsiteID == entry.WebsiteID && e.Environment == entry.Environment {
			if e.Site == entry.Site && e.WebsiteRevision == entry.WebsiteRevision && e.ReportingOrigin == entry.ReportingOrigin && e.Status == "connected" && entry.Status == "connected" {
				var err error
				entry, err = mergeMeasurements(e, entry, now)
				if err != nil {
					return err
				}
			}
			continue
		}
		next := e
		next.Samples = []Sample{}
		for _, sample := range e.Samples {
			at, _ := time.Parse(time.RFC3339Nano, sample.ReceivedAt)
			if !at.Before(windowStart(now)) {
				next.Samples = append(next.Samples, sample)
			}
		}
		entries = append(entries, next)
	}
	entries = append(entries, entry)
	next := diskState{Version: 1, OrganizationID: r.state.OrganizationID, Entries: entries}
	if !validState(next, r.state.OrganizationID) {
		return ErrCapacity
	}
	err := r.persist(next)
	if err == nil || errors.Is(err, errCommitted) {
		r.state = next
	}
	return err
}
func mergeMeasurements(prior, next Export, now time.Time) (Export, error) {
	measurements := map[string]Sample{}
	pages := map[string]Sample{}
	for _, e := range []Export{prior, next} {
		for _, sample := range e.Samples {
			at, _ := time.Parse(time.RFC3339Nano, sample.ReceivedAt)
			if at.Before(windowStart(now)) {
				continue
			}
			if existing, ok := pages[sample.SampleID]; ok && (existing.Path != sample.Path || existing.Viewport != sample.Viewport) {
				return Export{}, ErrInvalid
			}
			pages[sample.SampleID] = sample
			key := sample.SampleID + ":" + sample.Metric
			if existing, ok := measurements[key]; ok {
				if existing.Sequence == sample.Sequence && existing != sample {
					return Export{}, ErrInvalid
				}
				if existing.Sequence >= sample.Sequence {
					continue
				}
			}
			measurements[key] = sample
		}
	}
	if len(measurements) > maxSamples {
		return Export{}, ErrCapacity
	}
	merged := next
	merged.Samples = make([]Sample, 0, len(measurements))
	for _, sample := range measurements {
		merged.Samples = append(merged.Samples, sample)
	}
	return normalize(merged, websites.Website{ID: next.WebsiteID, Site: next.Site, Revision: next.WebsiteRevision}, next.Environment, now)
}
func (r *repository) prune(now time.Time) error {
	changed := false
	entries := make([]Export, 0, len(r.state.Entries))
	for _, e := range r.state.Entries {
		next := e
		next.Samples = []Sample{}
		for _, s := range e.Samples {
			at, _ := time.Parse(time.RFC3339Nano, s.ReceivedAt)
			if !at.Before(windowStart(now)) {
				next.Samples = append(next.Samples, s)
			} else {
				changed = true
			}
		}
		entries = append(entries, next)
	}
	if !changed {
		return nil
	}
	next := diskState{Version: 1, OrganizationID: r.state.OrganizationID, Entries: entries}
	err := r.persist(next)
	if err == nil || errors.Is(err, errCommitted) {
		r.state = next
	}
	return err
}
func (r *repository) persist(state diskState) error {
	if !privateDir(filepath.Dir(r.path)) {
		return ErrInvalid
	}
	if info, err := os.Lstat(r.path); err == nil {
		if !info.Mode().IsRegular() || info.Mode().Perm() != 0600 {
			return ErrInvalid
		}
	} else if !os.IsNotExist(err) {
		return err
	}
	raw, err := json.Marshal(state)
	if err != nil || len(raw) > maxBytes {
		return ErrCapacity
	}
	f, err := os.CreateTemp(filepath.Dir(r.path), ".web-vitals-*")
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
	return syncCommittedDirectory(filepath.Dir(r.path))
}

// The new file is already committed after Rename. Every failure in this final
// durability step must keep the repository's in-memory copy aligned with disk.
func syncCommittedDirectory(path string) error {
	dir, err := os.Open(path)
	if err != nil {
		return fmt.Errorf("%w: %v", errCommitted, err)
	}
	defer dir.Close()
	if err = dir.Sync(); err != nil {
		return fmt.Errorf("%w: %v", errCommitted, err)
	}
	return nil
}
