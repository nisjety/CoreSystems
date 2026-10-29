package workspace

import (
	"context"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/siteintegrations"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

func OpenRegistry(path, org string, cfg pulse.Config) (*websites.Service, error) {
	returnURL, callbackURL, err := pulse.ResolveGoogleConsentURLs(cfg.GoogleConsentReturnURL, cfg.GoogleOAuthCallbackURL)
	if err != nil {
		return nil, err
	}
	repo, err := websites.Open(path, org)
	if err != nil {
		return nil, err
	}
	broker := siteintegrations.New(siteintegrations.Config{IntegrationCoreURL: cfg.IntegrationCoreURL, BearerTokenFile: cfg.IntegrationBearerTokenFile, PreferredGoogleConnectionID: cfg.GoogleConnectionID, GoogleConsentReturnURL: returnURL, GoogleOAuthCallbackURL: callbackURL})
	mapEvidence := func(value siteintegrations.Mapping) websites.Access {
		return websites.Access{Status: value.Status, ConnectionID: value.ConnectionID, Property: value.Property, AccessLevel: value.AccessLevel, PropertyID: value.PropertyID, StreamID: value.StreamID, MeasurementID: value.MeasurementID, ReportingSite: value.ReportingSite, CheckedAt: value.CheckedAt}
	}
	registry := websites.NewService(repo, func(ctx context.Context, site string) websites.Evidence {
		if !boundToOrganization(cfg.IntegrationBearerTokenFile, org) {
			return websites.Evidence{SearchConsole: websites.Access{Status: "source_unavailable"}, GA4: websites.Access{Status: "source_unavailable"}}
		}
		evidence := broker.Discover(ctx, site)
		return websites.Evidence{SearchConsole: mapEvidence(evidence.SearchConsole), GA4: mapEvidence(evidence.GA4), ConnectionsFingerprint: evidence.ConnectionsFingerprint}
	}, time.Now)
	registry.SetResourceResolver(func(ctx context.Context, site string, a websites.Access) (websites.Access, error) {
		m, err := broker.ResolveGoogleResource(ctx, site, siteintegrations.Mapping{Status: a.Status, ConnectionID: a.ConnectionID, Property: a.Property, AccessLevel: a.AccessLevel, PropertyID: a.PropertyID, StreamID: a.StreamID, MeasurementID: a.MeasurementID, ReportingSite: a.ReportingSite, CheckedAt: a.CheckedAt})
		return mapEvidence(m), err
	})
	registry.SetResourceActions(func(ctx context.Context, site string) (websites.ResourceReport, error) {
		if !boundToOrganization(cfg.IntegrationBearerTokenFile, org) {
			return websites.ResourceReport{}, websites.ErrUnavailable
		}
		v, err := broker.GoogleResources(ctx, site)
		if err != nil {
			return websites.ResourceReport{}, err
		}
		out := websites.ResourceReport{Status: v.Status, Partial: v.Partial, CheckedAt: v.CheckedAt, Choices: []websites.ResourceChoice{}}
		for _, c := range v.Items {
			out.Choices = append(out.Choices, websites.ResourceChoice{ID: c.ID, Provider: c.Provider, Label: c.Label, Access: mapEvidence(c.Mapping)})
		}
		return out, nil
	})
	registry.SetGoogleActions(websites.GoogleActions{
		Begin: func(ctx context.Context) (websites.GoogleSession, error) {
			if !boundToOrganization(cfg.IntegrationBearerTokenFile, org) {
				return websites.GoogleSession{}, websites.ErrUnavailable
			}
			value, err := broker.BeginGoogle(ctx, returnURL)
			return websites.GoogleSession{SessionID: value.SessionID, AuthorizationURL: value.AuthorizationURL, ExpiresAt: value.ExpiresAt}, err
		},
		Status: func(ctx context.Context, id string) (string, error) {
			if !boundToOrganization(cfg.IntegrationBearerTokenFile, org) {
				return "", websites.ErrUnavailable
			}
			return broker.GoogleStatus(ctx, id)
		},
	})
	return registry, nil
}
