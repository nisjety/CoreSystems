package siteaudit

import (
	"bytes"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"sync"
)

type diskState struct {
	Version        int             `json:"version"`
	OrganizationID string          `json:"organizationId"`
	Runs           []Snapshot      `json:"runs"`
	Monitoring     []monitorRecord `json:"monitoring,omitempty"`
	InsightVersion int             `json:"insightVersion,omitempty"`
	Insights       []insightRecord `json:"insights"`
}

// Repository is intentionally single-process, like the current website/work
// registries. Audit receipts are bounded; tasks preserve their own evidence.
type Repository struct {
	mu            sync.Mutex
	path, org     string
	state         diskState
	syncDir       func(string) error
	beforePersist func(diskState) error
}

// Rename committed the exact bytes even if the parent-directory durability
// barrier subsequently failed. Callers must not roll memory back to stale data.
var errCommitted = errors.New("audit transaction committed but directory sync failed")

func Open(path, org string) (*Repository, error) {
	if !filepath.IsAbs(path) || filepath.Clean(path) != path || !orgPattern.MatchString(org) {
		return nil, ErrInvalid
	}
	dir, err := os.Lstat(filepath.Dir(path))
	if err != nil || !dir.IsDir() || dir.Mode().Perm() != 0700 {
		return nil, errors.New("audit history requires a private directory")
	}
	r := &Repository{path: path, org: org, state: diskState{Version: 1, OrganizationID: org, Runs: []Snapshot{}, InsightVersion: 1, Insights: []insightRecord{}}}
	info, err := os.Lstat(path)
	if os.IsNotExist(err) {
		if err = r.persist(r.state); err != nil {
			return nil, err
		}
		return r, nil
	}
	if err != nil || !info.Mode().IsRegular() || info.Mode().Perm() != 0600 || info.Size() > maxFileBytes {
		return nil, errors.New("audit history requires a protected regular file")
	}
	f, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	current, err := f.Stat()
	if err != nil || !os.SameFile(info, current) {
		return nil, ErrInvalid
	}
	raw, err := io.ReadAll(io.LimitReader(f, maxFileBytes+1))
	if err != nil || len(raw) > maxFileBytes {
		return nil, ErrInvalid
	}
	d := json.NewDecoder(bytes.NewReader(raw))
	d.DisallowUnknownFields()
	var state diskState
	if d.Decode(&state) != nil || d.Decode(new(any)) != io.EOF || state.OrganizationID != org || validateState(state) != nil {
		return nil, errors.New("audit history is corrupt or belongs to another organization")
	}
	changed := false
	if state.InsightVersion == 0 {
		state, err = bootstrapInsights(state)
		if err != nil {
			return nil, err
		}
		changed = true
	}
	for i, v := range state.Runs {
		if v.Status == "running" {
			v.Status = "failed"
			v.Reason = "interrupted"
			v.CompletedAt = v.StartedAt
			state, err = recordInsights(state, v)
			if err != nil {
				return nil, err
			}
			state.Runs[i] = v
			state = completeMonitor(state, v)
			changed = true
		}
	}
	if changed {
		state, err = fitInsightBudget(state, "")
		if err != nil {
			return nil, err
		}
		if err = r.persist(state); err != nil {
			return nil, err
		}
	}
	r.state = state
	return r, nil
}
func validateState(state diskState) error {
	if state.Version != 1 || !orgPattern.MatchString(state.OrganizationID) || state.Runs == nil || len(state.Runs) > maxRuns {
		return ErrInvalid
	}
	if err := validateMonitors(state.Monitoring); err != nil {
		return err
	}
	if err := validateInsights(state.InsightVersion, state.Insights); err != nil {
		return err
	}
	ids := map[string]bool{}
	sites := map[string]int{}
	for _, v := range state.Runs {
		if ids[v.ID] || validateSnapshot(v) != nil {
			return ErrInvalid
		}
		ids[v.ID] = true
		sites[v.WebsiteID]++
		if sites[v.WebsiteID] > maxSiteRuns {
			return ErrInvalid
		}
	}
	for _, scheduled := range state.Monitoring {
		if scheduled.LastOutcome != "running" {
			continue
		}
		found := false
		for _, run := range state.Runs {
			if run.ID == scheduled.LastAuditID && run.Status == "running" && run.WebsiteID == scheduled.WebsiteID && run.Site == scheduled.Site && run.WebsiteRevision == scheduled.WebsiteRevision {
				found = true
				break
			}
		}
		if !found {
			return ErrInvalid
		}
	}
	if err := validateInsightProofs(state); err != nil {
		return err
	}
	return nil
}
func cloneState(s diskState) diskState {
	s.Runs = append([]Snapshot{}, s.Runs...)
	for i, v := range s.Runs {
		s.Runs[i] = copySnapshot(v)
	}
	s.Monitoring = append([]monitorRecord{}, s.Monitoring...)
	for i, v := range s.Monitoring {
		s.Monitoring[i] = cloneMonitor(v)
	}
	s.Insights = append([]insightRecord{}, s.Insights...)
	for i, v := range s.Insights {
		s.Insights[i] = cloneInsight(v)
	}
	return s
}
func (r *Repository) list(id, site string) []Snapshot {
	r.mu.Lock()
	defer r.mu.Unlock()
	out := []Snapshot{}
	for i := len(r.state.Runs) - 1; i >= 0; i-- {
		v := r.state.Runs[i]
		if v.WebsiteID == id && v.Site == site {
			out = append(out, copySnapshot(v))
		}
	}
	return out
}
func (r *Repository) save(v Snapshot) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	next := cloneState(r.state)
	if err := validateSnapshot(v); err != nil {
		return err
	}
	next, err := recordInsights(next, v)
	if err != nil {
		return err
	}
	next, err = putSnapshot(next, v)
	if err != nil {
		return err
	}
	next = completeMonitor(next, v)
	next, err = fitInsightBudget(next, v.ID)
	if err != nil {
		return err
	}
	return r.commit(next)
}
func putSnapshot(next diskState, v Snapshot) (diskState, error) {
	for i, old := range next.Runs {
		if old.ID == v.ID {
			if old.WebsiteID != v.WebsiteID || old.Site != v.Site || old.WebsiteRevision != v.WebsiteRevision {
				return diskState{}, ErrConflict
			}
			next.Runs[i] = copySnapshot(v)
			return next, nil
		}
	}
	for {
		same := 0
		for _, old := range next.Runs {
			if old.WebsiteID == v.WebsiteID {
				same++
			}
		}
		if len(next.Runs) < maxRuns && same < maxSiteRuns {
			break
		}
		remove := -1
		for i, old := range next.Runs {
			if old.Status != "running" && (same < maxSiteRuns || old.WebsiteID == v.WebsiteID) {
				remove = i
				break
			}
		}
		if remove < 0 {
			return diskState{}, ErrCapacity
		}
		next.Runs = append(append([]Snapshot{}, next.Runs[:remove]...), next.Runs[remove+1:]...)
	}
	next.Runs = append(next.Runs, copySnapshot(v))
	return next, nil
}
func (r *Repository) commit(next diskState) error {
	if err := validateState(next); err != nil {
		return err
	}
	err := r.persist(next)
	if err == nil || errors.Is(err, errCommitted) {
		r.state = cloneState(next)
	}
	return err
}
func (r *Repository) persist(state diskState) error {
	if r.beforePersist != nil {
		if err := r.beforePersist(cloneState(state)); err != nil {
			return err
		}
	}
	raw, err := json.Marshal(state)
	if err != nil {
		return err
	}
	if len(raw) > maxFileBytes {
		return ErrCapacity
	}
	f, err := os.CreateTemp(filepath.Dir(r.path), ".audit-*")
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
	if err = os.Rename(name, r.path); err != nil {
		return err
	}
	if r.syncDir != nil {
		if err = r.syncDir(filepath.Dir(r.path)); err != nil {
			return errCommitted
		}
		return nil
	}
	dir, err := os.Open(filepath.Dir(r.path))
	if err != nil {
		return errCommitted
	}
	defer dir.Close()
	if err = dir.Sync(); err != nil {
		return errCommitted
	}
	return nil
}
