package websites

import (
	"context"
	"regexp"
)

var resourcePattern = regexp.MustCompile(`^res_[a-f0-9]{32}$`)

type ResourceChoice struct {
	ID       string `json:"id"`
	Provider string `json:"provider"`
	Label    string `json:"label"`
	Access   Access `json:"-"`
}
type ResourceReport struct {
	WebsiteID       string           `json:"websiteId"`
	Site            string           `json:"site"`
	WebsiteRevision int64            `json:"websiteRevision"`
	Status          string           `json:"status"`
	Partial         bool             `json:"partial"`
	CheckedAt       string           `json:"checkedAt"`
	Choices         []ResourceChoice `json:"choices"`
}
type ResourceActions func(context.Context, string) (ResourceReport, error)

func (s *Service) SetResourceResolver(value func(context.Context, string, Access) (Access, error)) {
	s.resourceResolver = value
}
func (s *Service) SetResourceActions(value ResourceActions) { s.resources = value }
func (s *Service) GoogleResources(ctx context.Context, id string) (ResourceReport, error) {
	r, err := s.Resolve(id, "")
	if err != nil {
		return ResourceReport{}, err
	}
	if s.resources == nil {
		return ResourceReport{}, ErrUnavailable
	}
	v, err := s.resources(ctx, r.Website.Site)
	if err != nil {
		return ResourceReport{}, ErrUnavailable
	}
	current, err := s.Resolve(id, r.Website.Site)
	if err != nil || current.Website.Revision != r.Website.Revision {
		return ResourceReport{}, ErrConflict
	}
	v.WebsiteID = id
	v.Site = r.Website.Site
	v.WebsiteRevision = r.Website.Revision
	return v, nil
}
func (s *Service) SelectGoogleResource(ctx context.Context, id, choice string, revision int64) (Website, error) {
	if revision < 1 || !resourcePattern.MatchString(choice) {
		return Website{}, ErrInvalid
	}
	r, err := s.Resolve(id, "")
	if err != nil {
		return Website{}, err
	}
	if r.Website.Revision != revision {
		return Website{}, ErrConflict
	}
	v, err := s.GoogleResources(ctx, id)
	if err != nil {
		return Website{}, err
	}
	for _, c := range v.Choices {
		if c.ID != choice {
			continue
		}
		if c.Access.Status != "connected" {
			return Website{}, ErrUnavailable
		}
		if s.resourceResolver != nil {
			resolved, err := s.resourceResolver(ctx, r.Website.Site, c.Access)
			if err != nil {
				return Website{}, ErrUnavailable
			}
			c.Access = resolved
		}
		return s.update(id, revision, func(next Record) (Record, error) {
			switch c.Provider {
			case "search_console":
				next.GSCChoiceID = choice
				next.GSCConnectionID = c.Access.ConnectionID
				next.Website.Integrations.SearchConsole = mapAccess(c.Access, false, s.stamp())
			case "ga4":
				next.GA4ChoiceID = choice
				next.GA4ConnectionID = c.Access.ConnectionID
				next.Website.Integrations.GA4 = mapAccess(c.Access, true, s.stamp())
			default:
				return Record{}, ErrInvalid
			}
			if !validMappings(next) {
				return Record{}, ErrUnavailable
			}
			return next, nil
		})
	}
	return Website{}, ErrInvalid
}
func applySelections(r Record, e *Evidence, resources ResourceReport, err error) {
	for _, item := range []struct {
		id, provider string
		mapping      *Access
	}{{r.GSCChoiceID, "search_console", &e.SearchConsole}, {r.GA4ChoiceID, "ga4", &e.GA4}} {
		if item.id == "" {
			continue
		}
		*item.mapping = Access{Status: "source_unavailable"}
		if err != nil {
			continue
		}
		found := false
		for _, choice := range resources.Choices {
			if choice.ID == item.id && choice.Provider == item.provider {
				*item.mapping = choice.Access
				found = true
				break
			}
		}
		if !found && !resources.Partial {
			item.mapping.Status = "not_found"
		}
	}
}
