package workqueue

import "time"

// AssigneeID is a Control Plane principal, validated by the authenticated BFF
// against live org membership before this trusted internal boundary is called.
type Planning struct {
	AssigneeID string `json:"assigneeId"`
	DueDate    string `json:"dueDate"`
	Priority   string `json:"priority"`
}

func validPlanning(v *Planning) bool {
	if v == nil {
		return true
	}
	if v.AssigneeID != "" && !orgPattern.MatchString(v.AssigneeID) {
		return false
	}
	if v.Priority != "low" && v.Priority != "normal" && v.Priority != "high" && v.Priority != "urgent" {
		return false
	}
	if v.DueDate != "" {
		d, err := time.Parse("2006-01-02", v.DueDate)
		if err != nil || d.Format("2006-01-02") != v.DueDate {
			return false
		}
	}
	return true
}
func (s *Service) Plan(id, site string, websiteRevision int64, taskID string, revision int64, p Planning) (Snapshot, error) {
	if websiteRevision < 1 || revision < 1 || !taskPattern.MatchString(taskID) || !validPlanning(&p) {
		return Snapshot{}, ErrInvalid
	}
	if _, err := s.binding(id, site, websiteRevision); err != nil {
		return Snapshot{}, err
	}
	if err := s.repo.plan(id, site, taskID, revision, p, s.now().UTC().Format(time.RFC3339Nano)); err != nil {
		return Snapshot{}, err
	}
	return s.List(id, site)
}
func (r *Repository) plan(websiteID, site, id string, revision int64, p Planning, stamp string) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	next := cloneState(r.state)
	for i, t := range next.Tasks {
		if t.ID != id || t.WebsiteID != websiteID || t.Site != site {
			continue
		}
		if t.Revision != revision {
			return ErrConflict
		}
		if t.Planning != nil && *t.Planning == p {
			return nil
		}
		t.Planning = &p
		t.Revision++
		t.UpdatedAt = stamp
		next.Tasks[i] = t
		next.Activity = appendActivity(next.Activity, Activity{ID: newID("evt_"), TaskID: id, Kind: "planning_changed", Status: t.Status, OccurredAt: stamp, Planning: &p})
		return r.commit(next)
	}
	return ErrNotFound
}
