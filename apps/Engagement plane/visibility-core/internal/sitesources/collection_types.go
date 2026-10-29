package sitesources

import (
	"context"
	"regexp"
	"time"
)

// Enrollment is local mapping metadata, never a provider grant or credential.
type Enrollment struct {
	WebsiteID       string
	Site            string
	WebsiteRevision int64
	Fingerprint     string
}
type enrollmentSource interface{ Enrollments() []Enrollment }
type CollectionState struct {
	WebsiteID       string `json:"websiteId"`
	Site            string `json:"site"`
	WebsiteRevision int64  `json:"websiteRevision"`
	Provider        string `json:"provider"`
	Status          string `json:"status"`
	NextRunAt       string `json:"nextRunAt,omitempty"`
	LastAttemptAt   string `json:"lastAttemptAt,omitempty"`
	LastSuccessAt   string `json:"lastSuccessAt,omitempty"`
	Reason          string `json:"reason,omitempty"`
}
type collectionJob struct {
	CollectionState
	Fingerprint string `json:"fingerprint"`
	RunID       string `json:"runId"`
	Attempts    int    `json:"attempts"`
}
type collectionDisk struct {
	Version        int             `json:"version"`
	OrganizationID string          `json:"organizationId"`
	Entries        []collectionJob `json:"entries"`
}

var fingerprintPattern = regexp.MustCompile(`^[a-f0-9]{64}$`)

func collectionBlank(e Enrollment) CollectionState {
	return CollectionState{WebsiteID: e.WebsiteID, Site: e.Site, WebsiteRevision: e.WebsiteRevision, Provider: "algolia", Status: "idle"}
}
func sameEnrollment(a, b Enrollment) bool { return a == b }
func jobEnrollment(j collectionJob) Enrollment {
	return Enrollment{j.WebsiteID, j.Site, j.WebsiteRevision, j.Fingerprint}
}
func (s *Service) enrollments() []Enrollment {
	owner, ok := s.oauth.(enrollmentSource)
	if !ok {
		return nil
	}
	return owner.Enrollments()
}
func (s *Service) enrollment(id, site string, revision int64) (Enrollment, error) {
	r, err := s.resolve(id, site, "algolia")
	if err != nil {
		return Enrollment{}, err
	}
	if revision > 0 && revision != r.Website.Revision {
		return Enrollment{}, ErrInvalid
	}
	for _, e := range s.enrollments() {
		if e.WebsiteID == id && e.Site == site && e.WebsiteRevision == r.Website.Revision && fingerprintPattern.MatchString(e.Fingerprint) {
			return e, nil
		}
	}
	return Enrollment{}, ErrInvalid
}
func (s *Service) currentEnrollment(e Enrollment) bool {
	current, err := s.enrollment(e.WebsiteID, e.Site, e.WebsiteRevision)
	return err == nil && sameEnrollment(current, e)
}
func collectionTime(v string) time.Time { t, _ := time.Parse(time.RFC3339Nano, v); return t }
func (s *Service) collectionAccess(ctx context.Context, e Enrollment) (Authorization, error) {
	r, err := s.resolve(e.WebsiteID, e.Site, "algolia")
	if err != nil {
		return Authorization{}, err
	}
	a, err := s.authorize(ctx, r, "algolia")
	if err != nil || !s.currentEnrollment(e) || !a.Managed || a.Fingerprint != e.Fingerprint {
		return Authorization{}, ErrUnavailable
	}
	return a, nil
}
