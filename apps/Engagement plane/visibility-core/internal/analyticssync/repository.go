package analyticssync

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sync"
)

var errCommitted = errors.New("analytics saved but directory sync failed")

// Repository is bounded, private and single-process/single-writer.
type Repository struct {
	mu        sync.Mutex
	path, org string
	state     diskState
	persistFn func(diskState) error
}

func Open(path, org string) (*Repository, error) {
	if !filepath.IsAbs(path) || filepath.Clean(path) != path || !orgPattern.MatchString(org) {
		return nil, ErrInvalid
	}
	if err := privateDirectory(filepath.Dir(path)); err != nil {
		return nil, err
	}
	r := &Repository{path: path, org: org, state: diskState{Version: 1, OrganizationID: org, Entries: []entry{}}}
	r.persistFn = r.persist
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
	if dec.Decode(&state) != nil || dec.Decode(new(any)) != io.EOF || state.OrganizationID != org || validateState(state) != nil {
		return nil, ErrInvalid
	}
	r.state = clone(state)
	return r, nil
}
func privateDirectory(path string) error {
	v, err := os.Lstat(path)
	if err != nil || !v.IsDir() || v.Mode().Perm() != 0700 {
		return errors.New("analytics snapshots require a private mode-0700 directory")
	}
	return nil
}
func (r *Repository) read() diskState { r.mu.Lock(); defer r.mu.Unlock(); return clone(r.state) }
func (r *Repository) update(fn func(diskState) (diskState, error)) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	next, err := fn(clone(r.state))
	if err != nil {
		return err
	}
	if validateState(next) != nil {
		return ErrInvalid
	}
	next, err = trimBudget(next, r.state)
	if err != nil {
		return err
	}
	err = r.persistFn(next)
	if err == nil || errors.Is(err, errCommitted) {
		r.state = clone(next)
	}
	return err
}

// Pruning is bounded by maxEntries, and never drops queued/running jobs or the
// entry changed by this transaction. A provider result is therefore saved once
// even when older retained observations must make room for it.
func trimBudget(next, prior diskState) (diskState, error) {
	unchanged := map[string]bool{}
	for _, e := range next.Entries {
		for _, p := range prior.Entries {
			if e.JobID != p.JobID {
				continue
			}
			a, _ := json.Marshal(e)
			b, _ := json.Marshal(p)
			unchanged[e.JobID] = bytes.Equal(a, b)
			break
		}
	}
	for range maxEntries + 1 {
		raw, err := json.Marshal(next)
		if err != nil {
			return next, ErrInvalid
		}
		if len(raw) <= maxBytes {
			return next, nil
		}
		oldest := -1
		for i, e := range next.Entries {
			if e.Status == "running" || e.Status == "queued" || !unchanged[e.JobID] {
				continue
			}
			if oldest < 0 || e.CompletedAt < next.Entries[oldest].CompletedAt {
				oldest = i
			}
		}
		if oldest < 0 {
			return next, ErrCapacity
		}
		next.Entries = append(next.Entries[:oldest:oldest], next.Entries[oldest+1:]...)
	}
	return next, ErrCapacity
}
func (r *Repository) persist(v diskState) error {
	if err := privateDirectory(filepath.Dir(r.path)); err != nil {
		return err
	}
	info, err := os.Lstat(r.path)
	if err == nil && (!info.Mode().IsRegular() || info.Mode().Perm() != 0600) {
		return ErrInvalid
	}
	if err != nil && !os.IsNotExist(err) {
		return err
	}
	raw, err := json.Marshal(v)
	if err != nil || len(raw) > maxBytes {
		return ErrCapacity
	}
	f, err := os.CreateTemp(filepath.Dir(r.path), ".analytics-*")
	if err != nil {
		return err
	}
	name := f.Name()
	defer os.Remove(name)
	if err = f.Chmod(0600); err == nil {
		_, err = f.Write(raw)
	}
	if err == nil {
		err = f.Sync()
	}
	closeErr := f.Close()
	if err == nil {
		err = closeErr
	}
	if err != nil {
		return err
	}
	dir, err := os.Open(filepath.Dir(r.path))
	if err != nil {
		return err
	}
	defer dir.Close()
	if err = os.Rename(name, r.path); err != nil {
		return err
	}
	if err = dir.Sync(); err != nil {
		return fmt.Errorf("%w: %v", errCommitted, err)
	}
	return nil
}
