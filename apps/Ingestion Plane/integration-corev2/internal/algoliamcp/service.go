package algoliamcp

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"github.com/triodelab/integration-corev2/internal/config"
	"github.com/triodelab/integration-corev2/internal/egress"
	"github.com/triodelab/integration-corev2/internal/sitesources"
	"github.com/triodelab/integration-corev2/internal/store"
	"net/http"
	"slices"
	"sort"
	"strings"
	"time"
)

type Service struct {
	diagnostic func(Diagnostic)
	repo       Repository
	tokens     Tokens
	http       *http.Client
	now        func() time.Time
	slots      chan struct{}
}

func New(repo Repository, tokens Tokens, client *http.Client, now func() time.Time, diagnostics ...func(Diagnostic)) *Service {
	if client == nil {
		client = egress.SafeClient(egress.ClientConfig{Guard: egress.New(), RequestTimeout: 8 * time.Second})
	}
	if now == nil {
		now = func() time.Time { return time.Now().UTC() }
	}
	var diagnostic func(Diagnostic)
	if len(diagnostics) == 1 {
		diagnostic = diagnostics[0]
	}
	return &Service{repo: repo, tokens: tokens, http: safeHTTP(client), now: now, slots: make(chan struct{}, 2), diagnostic: diagnostic}
}
func (s *Service) Access(ctx context.Context, org, id string) (Access, error) {
	if org == "" || !connectionPattern.MatchString(id) {
		return Access{}, ErrAccess
	}
	c, err := s.repo.GetConnection(ctx, id)
	if err != nil || c.OrganizationID != org || c.ProviderKey != "algolia" {
		return Access{}, ErrAccess
	}
	status := "reconnect_required"
	registrationValid := true
	if check, ok := s.tokens.(interface{ AlgoliaGrantMatchesRegistration(store.Connection) bool }); ok {
		registrationValid = check.AlgoliaGrantMatchesRegistration(c)
	}
	if registrationValid && c.DeletedAt == nil && c.Status == "active" && c.EncryptedAccessToken != "" && (c.AccessTokenExpiresAt.After(s.now()) || c.EncryptedRefreshToken != "") && c.ProviderContext["algolia_resource"] == config.AlgoliaMCPResource && slices.Contains(c.Capabilities, "search.analytics.read") && slices.Contains(c.Scopes, "public") {
		status = "active"
	}
	caps := append([]string{}, c.Capabilities...)
	scopes := append([]string{}, c.Scopes...)
	sort.Strings(caps)
	sort.Strings(scopes)
	// Grant authority, not credential material: ordinary refresh stays stable.
	material := strings.Join([]string{c.ID, c.CreatedAt.UTC().Format(time.RFC3339Nano), status, strings.Join(caps, ","), strings.Join(scopes, ","), c.ProviderContext["algolia_client_id"]}, "\n")
	sum := sha256.Sum256([]byte(material))
	return Access{ConnectionID: id, Status: status, Generation: hex.EncodeToString(sum[:16])}, nil
}
func (s *Service) open(ctx context.Context, org, id string) (*session, map[string]tool, func(), error) {
	return s.openBudget(ctx, org, id, 20*time.Second, 14)
}
func (s *Service) openBudget(ctx context.Context, org, id string, budget time.Duration, maxRPC int) (*session, map[string]tool, func(), error) {
	select {
	case s.slots <- struct{}{}:
	default:
		return nil, nil, nil, ErrBusy
	}
	release := func() { <-s.slots }
	ctx, cancel := context.WithTimeout(ctx, budget)
	done := func() { cancel(); release() }
	access, err := s.Access(ctx, org, id)
	if err != nil || access.Status != "active" {
		done()
		return nil, nil, nil, ErrAccess
	}
	token, err := s.tokens.AlgoliaAccessToken(ctx, org, id)
	if err != nil {
		done()
		return nil, nil, nil, ErrUnavailable
	}
	valid := func() bool {
		current, e := s.Access(ctx, org, id)
		return e == nil && current.Status == "active" && current.Generation == access.Generation
	}
	if !valid() {
		done()
		return nil, nil, nil, ErrAccess
	}
	rejected := func() {
		if !valid() {
			return
		}
		if sink, ok := s.tokens.(interface {
			InvalidateAlgoliaAccessToken(context.Context, string, string, string) error
		}); ok {
			_ = sink.InvalidateAlgoliaAccessToken(ctx, org, id, token.AccessToken)
		}
	}
	session := &session{http: s.http, token: token.AccessToken, valid: valid, ctx: ctx, unauthorized: rejected, diagnostic: s.diagnostic, maxRPC: maxRPC}
	if err = session.initialize(ctx); err != nil {
		done()
		return nil, nil, nil, err
	}
	tools, err := session.tools(ctx)
	if err != nil {
		done()
		return nil, nil, nil, err
	}
	return session, tools, done, nil
}
func (s *Service) Applications(ctx context.Context, org, id string) ([]Application, error) {
	session, tools, done, err := s.open(ctx, org, id)
	if err != nil {
		return nil, err
	}
	defer done()
	var items []Application
	if _, hosted := tools[discoverTools]; hosted {
		items, err = session.hostedApplications(ctx, tools)
	} else {
		items, err = applications(tools)
		if err != nil {
			s.applicationSchemaFailure(tools, session.catalog, nil)
		}
	}
	if !session.valid() {
		return nil, ErrAccess
	}
	return items, err
}
func (s *Service) Indices(ctx context.Context, org, id, app string) ([]Index, error) {
	if !appPattern.MatchString(app) {
		return nil, ErrAccess
	}
	session, tools, done, err := s.open(ctx, org, id)
	if err != nil {
		return nil, err
	}
	defer done()
	items, err := session.indices(ctx, tools, app)
	if !session.valid() {
		return nil, ErrAccess
	}
	return items, err
}
func (s *Service) Resources(ctx context.Context, org string, in ResourceInput) (Resources, error) {
	result := Resources{ConnectionID: in.ConnectionID, ApplicationID: in.ApplicationID, IndexName: in.IndexName, Status: "unavailable"}
	if !indexPattern.MatchString(in.IndexName) {
		return result, ErrAccess
	}
	indices, err := s.Indices(ctx, org, in.ConnectionID, in.ApplicationID)
	if err != nil {
		return result, err
	}
	for _, index := range indices {
		if index.Name == in.IndexName {
			result.Status = "allowed"
			return result, nil
		}
	}
	return result, nil
}
func (s *Service) Report(ctx context.Context, org string, in ReportInput) (sitesources.Report, error) {
	identity, _ := json.Marshal(sitesources.Input{WebsiteID: in.WebsiteID, Site: in.Site, WebsiteRevision: in.WebsiteRevision, Provider: "algolia"})
	if _, err := sitesources.ParseInput(identity); err != nil || !appPattern.MatchString(in.ApplicationID) || !indexPattern.MatchString(in.IndexName) {
		return sitesources.Report{}, ErrAccess
	}
	now := s.now().UTC()
	end := time.Date(now.Year(), now.Month(), now.Day()-1, 0, 0, 0, 0, time.UTC)
	window := sitesources.Window{StartDate: end.AddDate(0, 0, -6).Format(time.DateOnly), EndDate: end.Format(time.DateOnly)}
	report := sitesources.Report{WebsiteID: in.WebsiteID, Site: in.Site, WebsiteRevision: in.WebsiteRevision, Provider: "algolia", ObservedAt: now.Format(time.RFC3339Nano), Window: window, Status: "unavailable", Algolia: &sitesources.Algolia{IndexName: in.IndexName, Region: "provider", Analytics: sitesources.Analytics{Status: "unavailable"}, TopSearches: sitesources.UnavailableSearchTerms(), NoResultTerms: sitesources.UnavailableSearchTerms()}}
	session, tools, done, err := s.openBudget(ctx, org, in.ConnectionID, 30*time.Second, 18)
	if err != nil {
		return report, err
	}
	defer done()
	indices, err := session.indices(ctx, tools, in.ApplicationID)
	if err != nil {
		return report, err
	}
	allowed := false
	for _, index := range indices {
		allowed = allowed || index.Name == in.IndexName
	}
	if !allowed {
		return report, ErrAccess
	}
	metrics := session.collectAggregate(ctx, tools, in, window)
	top := session.collectTerms(ctx, tools, topSearches, in, window)
	withoutResults := session.collectTerms(ctx, tools, searchesNoResults, in, window)
	completed := s.now().UTC()
	if !session.valid() || completed.Format(time.DateOnly) != now.Format(time.DateOnly) {
		return report, ErrUnavailable
	}
	report.ObservedAt = completed.Format(time.RFC3339Nano)
	report.Algolia.Analytics = metrics
	report.Algolia.TopSearches = &top
	report.Algolia.NoResultTerms = &withoutResults
	if metrics.Status == "connected" || top.Status == "connected" || withoutResults.Status == "connected" {
		report.Status = "connected"
	}
	return report, nil
}
