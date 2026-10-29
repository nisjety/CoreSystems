package workqueue

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"slices"
	"sync"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
)

type diskState struct {
	Version        int        `json:"version"`
	OrganizationID string     `json:"organizationId"`
	Tasks          []Task     `json:"tasks"`
	Activity       []Activity `json:"activity"`
}

// Repository supports one process/replica, matching the current website registry.
type Repository struct {
	mu        sync.Mutex
	path, org string
	state     diskState
}

var errCommitted = errors.New("work transaction committed but directory sync failed")

func Open(path, org string) (*Repository, error) {
	if !filepath.IsAbs(path) || filepath.Clean(path) != path || !orgPattern.MatchString(org) {
		return nil, ErrInvalid
	}
	dir, err := os.Lstat(filepath.Dir(path))
	if err != nil || !dir.IsDir() || dir.Mode().Perm() != 0700 {
		return nil, errors.New("work queue requires a private mode-0700 directory")
	}
	r := &Repository{path: path, org: org, state: diskState{Version: 1, OrganizationID: org, Tasks: []Task{}, Activity: []Activity{}}}
	info, err := os.Lstat(path)
	if os.IsNotExist(err) {
		if err := r.persist(r.state); err != nil {
			return nil, err
		}
		return r, nil
	}
	if err != nil || !info.Mode().IsRegular() || info.Mode().Perm() != 0600 || info.Size() > maxFileBytes {
		return nil, errors.New("work queue requires a protected regular file")
	}
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	current, err := file.Stat()
	if err != nil || !os.SameFile(info, current) {
		return nil, ErrInvalid
	}
	raw, err := io.ReadAll(io.LimitReader(file, maxFileBytes+1))
	if err != nil || len(raw) > maxFileBytes {
		return nil, ErrInvalid
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	var state diskState
	if decoder.Decode(&state) != nil || decoder.Decode(new(any)) != io.EOF || state.OrganizationID != org || validateState(state) != nil {
		return nil, errors.New("work queue is corrupt or belongs to another organization")
	}
	r.state = cloneState(state)
	return r, nil
}
func cloneState(s diskState) diskState {
	s.Tasks = append([]Task{}, s.Tasks...)
	s.Activity = append([]Activity{}, s.Activity...)
	for i, t := range s.Tasks {
		s.Tasks[i] = cloneTask(t)
	}
	for i, e := range s.Activity {
		s.Activity[i] = cloneActivity(e)
	}
	return s
}
func validateState(s diskState) error {
	if s.Version != 1 || !orgPattern.MatchString(s.OrganizationID) || s.Tasks == nil || s.Activity == nil || len(s.Tasks) > maxTasks || len(s.Activity) > maxActivity {
		return ErrInvalid
	}
	tasks, findings, events := map[string]Task{}, map[string]bool{}, map[string]bool{}
	for _, t := range s.Tasks {
		canonical, err := pulse.CanonicalSite(t.Site)
		key := findingKey(t.WebsiteID, t.Site, t.Evidence)
		if err != nil || canonical != t.Site || !taskPattern.MatchString(t.ID) || !websitePattern.MatchString(t.WebsiteID) || tasks[t.ID].ID != "" || findings[key] || t.Revision < 1 || !validStatus(t.Status) || !validStamp(t.CreatedAt) || !validStamp(t.UpdatedAt) || !validEvidence(t.Evidence) || !validPlanning(t.Planning) {
			return ErrInvalid
		}
		if t.Verification != nil && !validVerification(t.Verification, t.Evidence) {
			return ErrInvalid
		}
		if (t.Evidence.Kind == "pagespeed_check" || t.Evidence.Kind == "site_audit_finding") && newer(t.Evidence.ObservedAt, t.CreatedAt) {
			return ErrInvalid
		}
		if t.Evidence.Kind == "site_audit_finding" && !auditSiteMatches(t.Site, t.Evidence.Audit) {
			return ErrInvalid
		}
		tasks[t.ID] = t
		findings[key] = true
	}
	for _, e := range s.Activity {
		if !eventPattern.MatchString(e.ID) || events[e.ID] || tasks[e.TaskID].ID == "" || !validStatus(e.Status) || !validStamp(e.OccurredAt) || (e.Kind != "created" && e.Kind != "status_changed" && e.Kind != "verified" && e.Kind != "planning_changed") {
			return ErrInvalid
		}
		if (e.Kind == "verified" && !validVerification(e.Verification, tasks[e.TaskID].Evidence)) || (e.Kind != "verified" && e.Verification != nil) {
			return ErrInvalid
		}
		if (e.Kind == "planning_changed") != (e.Planning != nil) || !validPlanning(e.Planning) {
			return ErrInvalid
		}
		events[e.ID] = true
	}
	return nil
}
func (r *Repository) snapshot(websiteID, site string) Snapshot {
	r.mu.Lock()
	defer r.mu.Unlock()
	result := Snapshot{WebsiteID: websiteID, Site: site, Tasks: []Task{}, Activity: []Activity{}}
	ids := map[string]bool{}
	for _, t := range r.state.Tasks {
		if t.WebsiteID == websiteID && t.Site == site {
			result.Tasks = append(result.Tasks, cloneTask(t))
			ids[t.ID] = true
		}
	}
	for _, e := range r.state.Activity {
		if ids[e.TaskID] {
			result.Activity = append(result.Activity, cloneActivity(e))
		}
	}
	slices.Reverse(result.Activity)
	result.Summary = summarize(result.Tasks)
	return result
}
func (r *Repository) create(websiteID, site string, evidence Evidence, stamp string) (bool, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	for _, task := range r.state.Tasks {
		if findingKey(task.WebsiteID, task.Site, task.Evidence) == findingKey(websiteID, site, evidence) {
			return false, nil
		}
	}
	if len(r.state.Tasks) >= maxTasks {
		return false, ErrCapacity
	}
	next := cloneState(r.state)
	task := cloneTask(Task{ID: newID("task_"), WebsiteID: websiteID, Site: site, Revision: 1, Status: "todo", CreatedAt: stamp, UpdatedAt: stamp, Evidence: evidence})
	next.Tasks = append(next.Tasks, task)
	next.Activity = appendActivity(next.Activity, Activity{ID: newID("evt_"), TaskID: task.ID, Kind: "created", Status: task.Status, OccurredAt: stamp})
	return true, r.commit(next)
}
func appendActivity(events []Activity, event Activity) []Activity {
	result := append(events, event)
	if len(result) > maxActivity {
		return append([]Activity{}, result[len(result)-maxActivity:]...)
	}
	return result
}
func (r *Repository) change(websiteID, site, id string, revision int64, status, stamp string) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	next := cloneState(r.state)
	for i, task := range next.Tasks {
		if task.ID != id || task.WebsiteID != websiteID || task.Site != site {
			continue
		}
		if task.Revision != revision {
			return ErrConflict
		}
		if task.Status == status {
			return nil
		}
		task.Status = status
		task.Revision++
		task.UpdatedAt = stamp
		next.Tasks[i] = task
		next.Activity = appendActivity(next.Activity, Activity{ID: newID("evt_"), TaskID: id, Kind: "status_changed", Status: status, OccurredAt: stamp})
		return r.commit(next)
	}
	return ErrNotFound
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
	raw, err := json.Marshal(state)
	if err != nil || len(raw) > maxFileBytes {
		return ErrCapacity
	}
	file, err := os.CreateTemp(filepath.Dir(r.path), ".work-*")
	if err != nil {
		return err
	}
	name := file.Name()
	defer os.Remove(name)
	if err = file.Chmod(0600); err == nil {
		_, err = file.Write(raw)
	}
	if err == nil {
		err = file.Sync()
	}
	closeErr := file.Close()
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
