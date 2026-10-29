package main

import (
	"context"
	"errors"
	"fmt"
	"github.com/triodelab/engagement-plane/visibility-core/internal/competitors"
	"github.com/triodelab/engagement-plane/visibility-core/internal/outcomes"
	"github.com/triodelab/engagement-plane/visibility-core/internal/searchresearch"
	"github.com/triodelab/studio-plane/planeclient"
	"log"
	"net/http"
	"os"
	"os/signal"
	"path/filepath"
	"syscall"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/algoliamapping"
	"github.com/triodelab/engagement-plane/visibility-core/internal/analyticssync"
	"github.com/triodelab/engagement-plane/visibility-core/internal/config"
	"github.com/triodelab/engagement-plane/visibility-core/internal/discovery"
	apphttp "github.com/triodelab/engagement-plane/visibility-core/internal/http"
	"github.com/triodelab/engagement-plane/visibility-core/internal/labreports"
	"github.com/triodelab/engagement-plane/visibility-core/internal/orgcore"
	"github.com/triodelab/engagement-plane/visibility-core/internal/preparation"
	"github.com/triodelab/engagement-plane/visibility-core/internal/publicsignals"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/siteaudit"
	"github.com/triodelab/engagement-plane/visibility-core/internal/sitesources"
	"github.com/triodelab/engagement-plane/visibility-core/internal/trends"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"github.com/triodelab/engagement-plane/visibility-core/internal/webvitals"
	"github.com/triodelab/engagement-plane/visibility-core/internal/workqueue"
	"github.com/triodelab/engagement-plane/visibility-core/internal/workspace"
)

func main() {
	var automaticTrends *trends.Automatic
	var automaticTrendsDone chan struct{}
	cfg, err := config.Load()
	if err != nil {
		log.Fatalf("marketing-core: config: %v", err)
	}
	planes, err := planeclient.New(planeclient.ConfigFromEnvironment("marketing-core"), nil)
	if err != nil {
		log.Fatalf("marketing-core: cross-plane configuration invalid")
	}
	pulseConfig := pulse.Config{
		IntegrationCoreURL:         cfg.IntegrationCoreURL,
		GoogleConnectionID:         cfg.GoogleConnectionID,
		GoogleGSCProperty:          cfg.GoogleGSCProperty,
		IntegrationBearerTokenFile: cfg.IntegrationBearerTokenFile,
		GoogleConsentReturnURL:     cfg.GoogleConsentReturnURL,
		GoogleOAuthCallbackURL:     cfg.GoogleOAuthCallbackURL,
	}
	var source apphttp.PulseGetter = pulse.NewService(pulseConfig, nil, nil)
	var research *searchresearch.Service
	var researchDone chan struct{}
	var comparisons *competitors.Service
	var comparisonsDone chan struct{}
	var monitoring *siteaudit.Monitor
	var labs *labreports.Service
	var labsDone chan struct{}
	var preparing *preparation.Service
	var preparingDone chan struct{}
	var analytics *analyticssync.Service
	var analyticsDone chan struct{}
	var publicJobs *publicsignals.Service
	var publicDone chan struct{}
	var vitals *webvitals.Service
	var vitalsDone chan struct{}
	var siteSourcesJobs *sitesources.Service
	var siteSourcesDone chan struct{}
	var watchConnections func(context.Context)
	if cfg.WebsitesFile != "" {
		registry, err := workspace.OpenRegistry(cfg.WebsitesFile, cfg.OrganizationID, pulseConfig)
		if err != nil {
			log.Fatalf("marketing-core: website registry: %v", err)
		}
		orgCore, err := orgcore.NewClient(orgcore.Config{URL: cfg.OrgCoreURL, ServiceID: cfg.OrgCoreServiceID, Token: cfg.OrgCoreServiceToken, OrganizationID: cfg.OrganizationID}, nil)
		if err != nil {
			log.Fatalf("marketing-core: org-core: %v", err)
		}
		if orgCore != nil {
			registry.SetPrimarySource(func(ctx context.Context) (string, error) {
				return orgCore.PrimarySite(ctx, cfg.OrganizationID)
			})
		}
		if len(registry.List()) == 0 && cfg.InitialSite != "" {
			ctx, cancel := context.WithTimeout(context.Background(), 12*time.Second)
			_, _, err = registry.Add(ctx, cfg.InitialSite)
			cancel()
			if err != nil {
				log.Fatalf("marketing-core: initial website registration failed")
			}
		}
		workspaceSource := workspace.New(pulseConfig, registry)
		labRepository, err := labreports.Open(filepath.Join(filepath.Dir(cfg.WebsitesFile), "lab-reports.json"), cfg.OrganizationID)
		if err != nil {
			log.Fatal("visibility-core: durable Lighthouse storage unavailable")
		}
		labs = labreports.New(labRepository, registry, workspace.NewLabSource(workspaceSource), nil)
		workspaceSource = workspaceSource.WithLabReports(labs)
		outcomeSource, outcomeErr := outcomes.New(registry, os.Getenv("LEADS_OUTCOMES_URL"), os.Getenv("LEADS_OUTCOMES_READ_TOKEN_FILE"))
		if outcomeErr != nil {
			log.Fatal("marketing-core: outcome connection configuration invalid")
		}
		workspaceSource = workspaceSource.WithBusinessOutcomes(outcomeSource)
		vitals, err = webvitals.New(filepath.Join(filepath.Dir(cfg.WebsitesFile), "web-vitals.json"), cfg.OrganizationID, registry, workspace.NewWebVitalsCollector(pulseConfig, registry), nil)
		if err != nil {
			log.Fatalf("marketing-core: first-party performance storage unavailable")
		}
		workspaceSource = workspaceSource.WithWebVitals(vitals)
		algoliaMapping, err := algoliamapping.New(filepath.Join(filepath.Dir(cfg.WebsitesFile), "algolia-mappings.json"), cfg.OrganizationID, registry, workspace.NewAlgoliaMappingClient(pulseConfig, registry))
		if err != nil {
			log.Fatalf("marketing-core: Algolia mapping storage unavailable")
		}
		workspaceSource = workspaceSource.WithAlgoliaMapping(algoliaMapping)
		siteSources, err := sitesources.NewWithOAuth(filepath.Join(filepath.Dir(cfg.WebsitesFile), "site-sources.json"), cfg.OrganizationID, registry, workspace.NewSiteSourcesCollector(pulseConfig, registry), algoliaMapping, nil)
		if err != nil {
			log.Fatalf("marketing-core: read-only site source storage unavailable")
		}
		workspaceSource = workspaceSource.WithSiteSources(siteSources)
		siteSourcesJobs = siteSources
		analyticsRepository, err := analyticssync.Open(filepath.Join(filepath.Dir(cfg.WebsitesFile), "analytics.json"), cfg.OrganizationID)
		if err != nil {
			log.Fatalf("marketing-core: analytics snapshot storage unavailable")
		}
		authority := workspace.NewSnapshotAuthority(pulseConfig, registry)
		researchRepository, err := searchresearch.Open(filepath.Join(filepath.Dir(cfg.WebsitesFile), "search-research.json"), cfg.OrganizationID)
		if err != nil {
			log.Fatal("visibility-core: search research storage unavailable")
		}
		research = searchresearch.New(researchRepository, registry, workspaceSource, authority, nil)
		workspaceSource = workspaceSource.WithSearchResearch(research)
		analytics = analyticssync.New(analyticsRepository, registry, workspace.NewSnapshotCollector(workspaceSource, authority), authority, nil)
		workspaceSource = workspaceSource.WithAnalyticsSync(analytics)
		automaticTrends, err = trends.NewAutomatic(filepath.Join(filepath.Dir(cfg.WebsitesFile), "trends.json"), cfg.OrganizationID, registry, workspaceSource, nil)
		if err != nil {
			log.Fatalf("marketing-core: automatic Trends storage unavailable")
		}
		workspaceSource = workspaceSource.WithAutomaticTrends(automaticTrends)
		watchConnections = func(ctx context.Context) {
			workspace.RunConnections(ctx, pulseConfig, registry, func(w websites.Website) {
				q := fmt.Sprintf("Q%d", (int(time.Now().UTC().Month())-1)/3+1)
				_ = analytics.EnsureConnectionSync(w.ID, w.Site, w.Revision, q)
				if preparing != nil {
					if state, err := preparing.Get(w.ID, w.Site); err == nil && state.Status == "idle" {
						_, _ = preparing.Queue(w.ID, w.Site, w.Revision)
					}
				}
				if publicJobs != nil {
					_ = publicJobs.EnsureConnectionSync(w.ID, w.Site, w.Revision)
				}
			})
		}
		publicRepository, err := publicsignals.Open(filepath.Join(filepath.Dir(cfg.WebsitesFile), "public-signals.json"), cfg.OrganizationID)
		if err != nil {
			log.Fatalf("marketing-core: public signal storage unavailable")
		}
		publicJobs = publicsignals.New(publicRepository, registry, workspace.NewPublicSignalsCollector(pulseConfig, registry), nil)
		workspaceSource = workspaceSource.WithPublicSignals(publicJobs)
		workRepository, err := workqueue.Open(filepath.Join(filepath.Dir(cfg.WebsitesFile), "work.json"), cfg.OrganizationID)
		if err != nil {
			log.Fatalf("marketing-core: work queue storage unavailable")
		}
		quarry, err := discovery.NewClient(discovery.ClientConfig{URL: cfg.QuarryURL, TokenFile: cfg.QuarryBearerTokenFile, OrganizationID: cfg.OrganizationID}, nil)
		if err != nil {
			log.Fatalf("marketing-core: Quarry discovery configuration unavailable")
		}
		var capture discovery.Capture
		if quarry != nil {
			capture = quarry
		}
		auditRepository, err := siteaudit.Open(filepath.Join(filepath.Dir(cfg.WebsitesFile), "audits.json"), cfg.OrganizationID)
		if err != nil {
			log.Fatalf("marketing-core: website audit storage unavailable")
		}
		auditCapture, err := siteaudit.NewClient(discovery.ClientConfig{URL: cfg.QuarryURL, TokenFile: cfg.QuarryBearerTokenFile, OrganizationID: cfg.OrganizationID}, nil)
		if err != nil {
			log.Fatalf("marketing-core: bounded Quarry audit configuration unavailable")
		}
		var boundedCapture siteaudit.Capture
		if auditCapture != nil {
			boundedCapture = auditCapture
		}
		comparisonRepository, err := competitors.Open(filepath.Join(filepath.Dir(cfg.WebsitesFile), "competitors.json"), cfg.OrganizationID)
		if err != nil {
			log.Fatal("visibility-core: comparison storage unavailable")
		}
		comparisons = competitors.New(comparisonRepository, registry, boundedCapture, nil)
		workspaceSource = workspaceSource.WithCompetitors(comparisons)
		audits := siteaudit.New(auditRepository, registry, boundedCapture, nil)
		monitoring = siteaudit.NewMonitor(audits)
		discoveryService := discovery.New(registry, capture, nil)
		preparationRepository, err := preparation.Open(filepath.Join(filepath.Dir(cfg.WebsitesFile), "preparation.json"), cfg.OrganizationID)
		if err != nil {
			log.Fatalf("marketing-core: website preparation storage unavailable")
		}
		preparing = preparation.New(preparationRepository, registry, workspace.NewPreparationRunner(registry, discoveryService, workspaceSource), nil)
		source = workspaceSource.WithWorkQueue(workqueue.New(workRepository, registry, workspaceSource, nil).WithAudit(audits).WithResearch(research)).WithDiscovery(discoveryService).WithAudit(audits).WithMonitoring(monitoring).WithPreparation(preparing)
	}
	monitorContext, stopMonitoring := context.WithCancel(context.Background())
	defer stopMonitoring()

	if research != nil {
		researchDone = make(chan struct{})
		go func() { defer close(researchDone); research.Run(monitorContext) }()
	}
	if comparisons != nil {
		comparisonsDone = make(chan struct{})
		go func() { defer close(comparisonsDone); comparisons.Run(monitorContext) }()
	}
	if labs != nil {
		labsDone = make(chan struct{})
		go func() { defer close(labsDone); labs.Run(monitorContext) }()
	}
	if automaticTrends != nil {
		automaticTrendsDone = make(chan struct{})
		go func() { defer close(automaticTrendsDone); automaticTrends.Run(monitorContext) }()
	}
	if watchConnections != nil {
		go watchConnections(monitorContext)
	}
	if vitals != nil {
		vitalsDone = make(chan struct{})
		go func() { defer close(vitalsDone); vitals.Run(monitorContext) }()
	}
	if siteSourcesJobs != nil {
		siteSourcesDone = make(chan struct{})
		go func() { defer close(siteSourcesDone); siteSourcesJobs.Run(monitorContext) }()
	}
	if monitoring != nil {
		go monitoring.Run(monitorContext)
	}
	if preparing != nil {
		preparingDone = make(chan struct{})
		go func() { defer close(preparingDone); preparing.Run(monitorContext) }()
	}
	if analytics != nil {
		analyticsDone = make(chan struct{})
		go func() { defer close(analyticsDone); analytics.Run(monitorContext) }()
	}
	if publicJobs != nil {
		publicDone = make(chan struct{})
		go func() { defer close(publicDone); publicJobs.Run(monitorContext) }()
	}
	server := apphttp.NewServerWithPlanes(cfg.Port, cfg.InternalAPIKey, planes, source)
	errorsCh := make(chan error, 1)
	go func() {
		log.Printf("marketing-core: listening on :%d", cfg.Port)
		errorsCh <- server.Start()
	}()

	stop := make(chan os.Signal, 1)
	signal.Notify(stop, syscall.SIGINT, syscall.SIGTERM)
	select {
	case <-stop:
	case err := <-errorsCh:
		if err != nil && !errors.Is(err, http.ErrServerClosed) {
			log.Fatalf("marketing-core: server: %v", err)
		}
	}

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	stopMonitoring()
	if researchDone != nil {
		select {
		case <-researchDone:
		case <-ctx.Done():
			log.Print("visibility-core: search research shutdown deadline reached")
		}
	}
	if comparisonsDone != nil {
		select {
		case <-comparisonsDone:
		case <-ctx.Done():
			log.Print("visibility-core: comparison shutdown deadline reached")
		}
	}
	if labsDone != nil {
		select {
		case <-labsDone:
		case <-ctx.Done():
			log.Print("visibility-core: Lighthouse shutdown deadline reached")
		}
	}
	if automaticTrendsDone != nil {
		select {
		case <-automaticTrendsDone:
		case <-ctx.Done():
			log.Printf("marketing-core: automatic Trends shutdown deadline reached")
		}
	}
	defer cancel()
	if siteSourcesDone != nil {
		select {
		case <-siteSourcesDone:
		case <-ctx.Done():
			log.Printf("marketing-core: site source collection shutdown deadline reached")
		}
	}
	if err := server.Shutdown(ctx); err != nil {
		log.Printf("marketing-core: shutdown: %v", err)
	}
	if preparingDone != nil {
		select {
		case <-preparingDone:
		case <-ctx.Done():
			log.Printf("marketing-core: preparation shutdown deadline reached")
		}
	}
	if analyticsDone != nil {
		select {
		case <-analyticsDone:
		case <-ctx.Done():
			log.Printf("marketing-core: analytics shutdown deadline reached")
		}
	}
	if publicDone != nil {
		select {
		case <-publicDone:
		case <-ctx.Done():
			log.Printf("marketing-core: public signal shutdown deadline reached")
		}
	}
	if vitalsDone != nil {
		select {
		case <-vitalsDone:
		case <-ctx.Done():
			log.Printf("marketing-core: first-party performance retention shutdown deadline reached")
		}
	}
}
