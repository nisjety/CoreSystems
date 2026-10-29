package algoliamcp

import (
	"context"

	"github.com/triodelab/integration-corev2/internal/sitesources"
)

func (s *session) collectAggregate(ctx context.Context, tools map[string]tool, in ReportInput, window sitesources.Window) sitesources.Analytics {
	failed := sitesources.Analytics{Status: "unavailable"}
	resolved, err := s.resolve(ctx, tools, noResults)
	if err != nil {
		return failed
	}
	args, err := analyticsArgs(resolved[noResults], in.ApplicationID, in.IndexName, window)
	if err != nil {
		_ = s.metaFailure("meta-inner-arguments", noResults, nil, resolved)
		return failed
	}
	raw, err := s.call(ctx, noResults, args)
	if err != nil {
		return failed
	}
	metrics := sitesources.ParseAlgoliaAggregate(raw, window)
	if metrics.Status != "connected" && s.resolved[noResults].indirect {
		_ = s.metaFailure("meta-read-result", noResults, raw, resolved)
	}
	return metrics
}
func (s *session) collectTerms(ctx context.Context, tools map[string]tool, operation string, in ReportInput, window sitesources.Window) sitesources.SearchTerms {
	failed := *sitesources.UnavailableSearchTerms()
	if operation != topSearches && operation != searchesNoResults {
		return failed
	}
	resolved, err := s.resolve(ctx, tools, operation)
	if err != nil {
		return failed
	}
	args, err := searchTermsArgs(resolved[operation], in.ApplicationID, in.IndexName, window)
	if err != nil {
		_ = s.metaFailure("meta-inner-arguments", operation, nil, resolved)
		return failed
	}
	raw, err := s.call(ctx, operation, args)
	if err != nil {
		return failed
	}
	terms := sitesources.ParseAlgoliaSearchTerms(raw)
	if terms.Status != "connected" {
		_ = s.metaFailure("meta-read-result", operation, raw, resolved)
	}
	return terms
}
func searchTermsArgs(t tool, app, index string, window sitesources.Window) (map[string]any, error) {
	key := "indexName"
	_, named := t.InputSchema.Properties[key]
	_, alternate := t.InputSchema.Properties["index"]
	if named == alternate {
		return nil, ErrUnavailable
	}
	if alternate {
		key = "index"
	}
	if !knownRequired(t.InputSchema, map[string]bool{"applicationId": true, key: true, "startDate": true, "endDate": true, "limit": true, "direction": true, "orderBy": true}) {
		return nil, ErrUnavailable
	}
	args := map[string]any{"applicationId": app, key: index, "startDate": window.StartDate, "endDate": window.EndDate}
	for name, value := range map[string]string{"direction": "desc", "orderBy": "searchCount"} {
		if p, exists := t.InputSchema.Properties[name]; exists {
			if p.Type != "string" {
				return nil, ErrUnavailable
			}
			args[name] = value
		}
	}
	if p, exists := t.InputSchema.Properties["limit"]; exists {
		if p.Type != "integer" {
			return nil, ErrUnavailable
		}
		args["limit"] = 20
	}
	if !verifiedArguments(t, args) {
		return nil, ErrUnavailable
	}
	return args, nil
}
