package searchresearch

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"reflect"
	"sync"
)

var errCommitted = errors.New("search research committed but directory sync failed")

// Repository is single-process/single-writer, like the current website registry.
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
	if err := protectedDirectory(filepath.Dir(path)); err != nil {
		return nil, err
	}
	r := &Repository{path: path, org: org, state: diskState{Version: 1, OrganizationID: org, Entries: []Entry{}}}
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
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	actual, err := file.Stat()
	if err != nil || !os.SameFile(info, actual) {
		return nil, ErrInvalid
	}
	raw, err := io.ReadAll(io.LimitReader(file, maxBytes+1))
	if err != nil || len(raw) > maxBytes {
		return nil, ErrInvalid
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	var state diskState
	if decoder.Decode(&state) != nil || decoder.Decode(new(any)) != io.EOF || state.OrganizationID != org || validState(state) != nil {
		return nil, ErrInvalid
	}
	r.state = cloneState(state)
	return r, nil
}
func protectedDirectory(path string) error {
	info, err := os.Lstat(path)
	if err != nil || !info.IsDir() || info.Mode().Perm() != 0700 {
		return errors.New("search research requires a private mode-0700 directory")
	}
	return nil
}
func (r *Repository) read() diskState { r.mu.Lock(); defer r.mu.Unlock(); return cloneState(r.state) }
func (r *Repository) update(fn func(diskState) (diskState, error)) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	next, err := fn(cloneState(r.state))
	if err != nil {
		return err
	}
	if validState(next) != nil {
		return ErrInvalid
	}
	if reflect.DeepEqual(next, r.state) {
		return nil
	}
	err = r.persistFn(next)
	if err == nil || errors.Is(err, errCommitted) {
		r.state = cloneState(next)
	}
	return err
}
func (r *Repository) persist(v diskState) error {
	if err := protectedDirectory(filepath.Dir(r.path)); err != nil {
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
	f, err := os.CreateTemp(filepath.Dir(r.path), ".search-research-*")
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
