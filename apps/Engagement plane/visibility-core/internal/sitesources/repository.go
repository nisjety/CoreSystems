package sitesources

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"time"
)

var errCommitted = errors.New("site sources committed but directory sync failed")

type repository struct {
	path  string
	state diskState
}

func privateDir(path string) bool {
	info, err := os.Lstat(path)
	return err == nil && info.IsDir() && info.Mode().Perm() == 0700
}
func open(path, org string) (*repository, error) {
	if !filepath.IsAbs(path) || filepath.Clean(path) != path || !orgPattern.MatchString(org) || !privateDir(filepath.Dir(path)) {
		return nil, ErrInvalid
	}
	r := &repository{path: path, state: diskState{Version: 1, OrganizationID: org, Entries: []Report{}}}
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
	if err != nil || len(raw) > maxBytes || !uniqueJSON(raw) {
		return nil, ErrInvalid
	}
	var state diskState
	d := json.NewDecoder(bytes.NewReader(raw))
	d.DisallowUnknownFields()
	if d.Decode(&state) != nil || d.Decode(new(any)) != io.EOF || !validState(state, org) {
		return nil, ErrInvalid
	}
	var shape struct {
		Entries []json.RawMessage `json:"entries"`
	}
	_ = json.Unmarshal(raw, &shape)
	for _, entry := range shape.Entries {
		if _, err = decodeReport(entry); err != nil {
			return nil, ErrInvalid
		}
	}
	r.state = state
	return r, nil
}
func validState(state diskState, org string) bool {
	if state.Version != 1 || state.OrganizationID != org || state.Entries == nil || len(state.Entries) > maxEntries {
		return false
	}
	keys := map[string]bool{}
	for _, entry := range state.Entries {
		key := entry.WebsiteID + ":" + entry.Provider
		if keys[key] || entry.Status == "not_loaded" || !validReport(entry) {
			return false
		}
		keys[key] = true
	}
	return true
}
func (r *repository) save(report Report, now time.Time) error {
	entries := make([]Report, 0, len(r.state.Entries)+1)
	for _, entry := range r.state.Entries {
		observed, _ := time.Parse(time.RFC3339Nano, entry.ObservedAt)
		if observed.After(now) || now.Sub(observed) >= Freshness || entry.Window != period(now) {
			continue
		}
		if entry.WebsiteID != report.WebsiteID || entry.Provider != report.Provider {
			entries = append(entries, entry)
		}
	}
	entries = append(entries, report)
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

// prune removes expired derived snapshots, never audit, work or history data.
func (r *repository) prune(now time.Time) error {
	entries := make([]Report, 0, len(r.state.Entries))
	for _, entry := range r.state.Entries {
		observed, _ := time.Parse(time.RFC3339Nano, entry.ObservedAt)
		if !observed.After(now) && now.Sub(observed) < Freshness && entry.Window == period(now) {
			entries = append(entries, entry)
		}
	}
	if len(entries) == len(r.state.Entries) {
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
	f, err := os.CreateTemp(filepath.Dir(r.path), ".site-sources-*")
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
