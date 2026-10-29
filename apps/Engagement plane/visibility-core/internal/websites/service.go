package websites

import (
	"context"
	"strings"
	"sync"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
)

type pendingSession struct {
	site, id string
	expires  time.Time
}
type Service struct {
	resources        ResourceActions
	resourceResolver func(context.Context, string, Access) (Access, error)
	primary          PrimarySource
	repo             *Repository
	discover         Discoverer
	now              func() time.Time
	google           GoogleActions
	sessionMu        sync.Mutex
	sessions         map[string]pendingSession
}

func NewService(repo *Repository, discover Discoverer, now func() time.Time) *Service {
	if now == nil {
		now = time.Now
	}
	return &Service{repo: repo, discover: discover, now: now, sessions: map[string]pendingSession{}}
}
func (s *Service) SetGoogleActions(actions GoogleActions) { s.google = actions }
func (s *Service) OrganizationID() string                 { return s.repo.org }
func (s *Service) List() []Website {
	result := []Website{}
	for _, record := range s.repo.List() {
		result = append(result, record.Website)
	}
	return result
}
func (s *Service) Resolve(id, site string) (Record, error) {
	for _, record := range s.repo.List() {
		if id != "" && record.Website.ID != id || id == "" && record.Website.Site != site {
			continue
		}
		if site != "" && record.Website.Site != site {
			return Record{}, ErrConflict
		}
		return record, nil
	}
	return Record{}, ErrNotFound
}
func (s *Service) Add(ctx context.Context, input string) (Website, bool, error) {
	site, err := canonicalInput(input)
	if err != nil {
		return Website{}, false, ErrInvalid
	}
	created := false
	var result Website
	err = s.repo.Update(func(records []Record) ([]Record, error) {
		for _, record := range records {
			if record.Website.Site == site {
				result = record.Website
				return records, nil
			}
		}
		if len(records) >= maxWebsites {
			return nil, ErrInvalid
		}
		result = newWebsite(site, s.stamp())
		created = true
		return append(records, Record{Website: result}), nil
	})
	if err != nil {
		return Website{}, false, err
	}
	if created {
		result, err = s.Verify(ctx, result.ID)
	}
	return result, created, err
}
func (s *Service) Change(ctx context.Context, id, input string, revision int64) (Website, error) {
	site, err := canonicalInput(input)
	if err != nil {
		return Website{}, ErrInvalid
	}
	changed := false
	result, err := s.update(id, revision, func(record Record) (Record, error) {
		if record.Website.Site == site {
			return record, nil
		}
		for _, other := range s.repo.records {
			if other.Website.ID != id && other.Website.Site == site {
				return Record{}, ErrConflict
			}
		}
		next := newWebsite(site, s.stamp())
		next.ID = record.Website.ID
		next.CreatedAt = record.Website.CreatedAt
		next.Revision = record.Website.Revision
		changed = true
		return Record{Website: next}, nil // Deliberately discard every binding and proof.
	})
	if err != nil {
		return Website{}, err
	}
	if changed {
		return s.Verify(ctx, id)
	}
	return result, nil
}
func (s *Service) Verify(ctx context.Context, id string) (Website, error) {
	record, err := s.Resolve(id, "")
	if err != nil {
		return Website{}, err
	}
	var selectedReport ResourceReport
	var selectedErr error
	var selectedDone chan struct{}
	if s.resources != nil && (record.GSCChoiceID != "" || record.GA4ChoiceID != "") {
		selectedDone = make(chan struct{})
		go func() { defer close(selectedDone); selectedReport, selectedErr = s.resources(ctx, record.Website.Site) }()
	}
	evidence := Evidence{SearchConsole: Access{Status: "needs_connection"}, GA4: Access{Status: "requires_consent"}}
	if s.discover != nil {
		bounded, cancel := context.WithTimeout(ctx, 10*time.Second)
		defer cancel()
		evidence = s.discover(bounded, record.Website.Site)
	}
	if selectedDone != nil {
		<-selectedDone
		applySelections(record, &evidence, selectedReport, selectedErr)
	}
	return s.update(id, record.Website.Revision, func(next Record) (Record, error) {
		if connectionsFingerprintPattern.MatchString(evidence.ConnectionsFingerprint) && evidence.SearchConsole.Status != "source_unavailable" && evidence.GA4.Status != "source_unavailable" {
			next.ConnectionsFingerprint = evidence.ConnectionsFingerprint
		}
		reportingSite := next.Website.Integrations.GA4.ReportingSite
		previousGA4 := next.Website.Integrations.GA4
		previousConnection := next.GA4ConnectionID
		next.GSCConnectionID = ""
		next.GA4ConnectionID = ""
		next.Website.Integrations.SearchConsole = mapAccess(evidence.SearchConsole, false, s.stamp())
		next.Website.Integrations.GA4 = mapAccess(evidence.GA4, true, s.stamp())
		if next.Website.Integrations.SearchConsole.Status == "connected" {
			next.GSCConnectionID = evidence.SearchConsole.ConnectionID
		}
		if next.Website.Integrations.GA4.Status == "connected" {
			next.GA4ConnectionID = evidence.GA4.ConnectionID
		}
		if next.Website.Ownership.Status != "dns_verified" {
			next.Website.Ownership = Ownership{Status: "unverified", CheckedAt: s.stamp()}
			if next.Website.Integrations.SearchConsole.AccessLevel == "siteOwner" && (next.Website.Integrations.SearchConsole.Property == next.Website.Site || strings.HasPrefix(next.Website.Integrations.SearchConsole.Property, "sc-domain:")) {
				next.Website.Ownership.Status = "google_owner"
			}
		}
		if reportingSite != "" && next.Website.Integrations.GA4.Status == "connected" && next.GA4ConnectionID == previousConnection && next.Website.Integrations.GA4.PropertyID == previousGA4.PropertyID && next.Website.Integrations.GA4.StreamID == previousGA4.StreamID {
			next.Website.Integrations.GA4.ReportingSite = reportingSite
		}
		if !validMappings(next) {
			return Record{}, ErrUnavailable
		}
		return next, nil
	})
}
func (s *Service) MapGA4ReportingSite(ctx context.Context, id, reportingSite string, revision int64) (Website, error) {
	record, err := s.Resolve(id, "")
	if err != nil {
		return Website{}, err
	}
	if record.Website.Revision != revision {
		return Website{}, ErrConflict
	}
	if !pulse.GA4ReportingSiteAllowed(record.Website.Site, reportingSite) {
		return Website{}, ErrInvalid
	}
	mapping := record.Website.Integrations.GA4
	if mapping.Status != "connected" || record.GA4ConnectionID == "" || s.discover == nil {
		return Website{}, ErrInvalid
	}
	bounded, cancel := context.WithTimeout(ctx, 8*time.Second)
	defer cancel()
	evidence := s.discover(bounded, record.Website.Site).GA4
	if bounded.Err() != nil || evidence.Status == "source_unavailable" {
		return Website{}, ErrUnavailable
	}
	if evidence.Status != "connected" || evidence.ConnectionID != record.GA4ConnectionID || evidence.PropertyID != mapping.PropertyID || evidence.StreamID != mapping.StreamID || evidence.MeasurementID != mapping.MeasurementID {
		return Website{}, ErrInvalid
	}
	return s.update(id, revision, func(record Record) (Record, error) {
		if reportingSite == record.Website.Site {
			record.Website.Integrations.GA4.ReportingSite = ""
			return record, nil
		}
		if record.GA4ConnectionID != evidence.ConnectionID || record.Website.Integrations.GA4.PropertyID != evidence.PropertyID || record.Website.Integrations.GA4.StreamID != evidence.StreamID {
			return Record{}, ErrInvalid
		}
		record.Website.Integrations.GA4.ReportingSite = reportingSite
		return record, nil
	})
}
func mapAccess(access Access, analytics bool, stamp string) Mapping {
	allowed := []string{"connected", "needs_connection", "not_found", "source_unavailable"}
	if analytics {
		allowed = []string{"connected", "requires_consent", "not_found", "needs_selection", "source_unavailable"}
	}
	if !statusIn(access.Status, allowed...) {
		return Mapping{Status: "source_unavailable", CheckedAt: stamp}
	}
	mapping := Mapping{Status: access.Status, CheckedAt: stamp}
	if mapping.Status == "connected" {
		if analytics {
			mapping.PropertyID = access.PropertyID
			mapping.StreamID = access.StreamID
			mapping.MeasurementID = access.MeasurementID
			mapping.ReportingSite = access.ReportingSite
		} else {
			mapping.Property = access.Property
			mapping.AccessLevel = access.AccessLevel
		}
	}
	return mapping
}
func (s *Service) MapClarity(id, projectID string, revision int64) (Website, error) {
	if !clarityPattern.MatchString(projectID) {
		return Website{}, ErrInvalid
	}
	return s.update(id, revision, func(record Record) (Record, error) {
		record.Website.Integrations.Clarity = Mapping{Status: "pending_verification", ProjectID: projectID}
		return record, nil
	})
}
func (s *Service) update(id string, revision int64, fn func(Record) (Record, error)) (Website, error) {
	var result Website
	err := s.repo.Update(func(records []Record) ([]Record, error) {
		for i, record := range records {
			if record.Website.ID != id {
				continue
			}
			if record.Website.Revision != revision {
				return nil, ErrConflict
			}
			next, err := fn(record)
			if err != nil {
				return nil, err
			}
			next.Website.Revision = record.Website.Revision + 1
			next.Website.UpdatedAt = s.stamp()
			records[i] = next
			result = next.Website
			return records, nil
		}
		return nil, ErrNotFound
	})
	return result, err
}
func (s *Service) stamp() string { return s.now().UTC().Format(time.RFC3339Nano) }

func canonicalInput(input string) (string, error) {
	value := strings.TrimSpace(input)
	if !strings.Contains(value, "://") {
		value = "https://" + value
	}
	return pulse.CanonicalSite(value)
}
