package temporal

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"
	"time"

	"github.com/rs/zerolog/log"
	"go.temporal.io/sdk/activity"

	"github.com/triodelab/quarry/internal/models"
	"github.com/triodelab/quarry/internal/modules"
	"github.com/triodelab/quarry/internal/nats"
	"github.com/triodelab/quarry/internal/pipeline"
	"github.com/triodelab/quarry/internal/scraper"
)

type Activities struct {
	scraper   *scraper.Scraper
	registry  *modules.Registry
	pipeline  *pipeline.Chain
	publisher *nats.SharedPublisher
}

// NewActivities creates activities with the default (non-persisting) pipeline.
func NewActivities(scraperEngine *scraper.Scraper, registry *modules.Registry) *Activities {
	return &Activities{
		scraper:  scraperEngine,
		registry: registry,
		pipeline: pipeline.NewDefaultChain(),
	}
}

// ActivitiesConfig provides optional dependencies for activities.
type ActivitiesConfig struct {
	Pipeline  *pipeline.Chain
	Publisher *nats.SharedPublisher
}

// NewActivitiesWithConfig creates activities with an explicit pipeline and NATS publisher.
func NewActivitiesWithConfig(scraperEngine *scraper.Scraper, registry *modules.Registry, cfg ActivitiesConfig) *Activities {
	p := cfg.Pipeline
	if p == nil {
		p = pipeline.NewDefaultChain()
	}
	return &Activities{
		scraper:   scraperEngine,
		registry:  registry,
		pipeline:  p,
		publisher: cfg.Publisher,
	}
}

func (a *Activities) FetchPageActivity(ctx context.Context, targetURL string) (string, error) {
	if strings.TrimSpace(targetURL) == "" {
		return "", fmt.Errorf("target url is required")
	}
	if a.scraper == nil {
		return "", fmt.Errorf("scraper is not initialized")
	}

	log.Info().Str("url", targetURL).Msg("FetchPageActivity: starting fetch")
	activity.RecordHeartbeat(ctx, "fetching")

	outputs, _, err := a.scraper.FetchFormats(ctx, targetURL, &scraper.FormatOptions{Formats: []string{"html"}})
	if err != nil {
		return "", err
	}
	html, _ := outputs["html"].(string)
	log.Info().Int("len", len(html)).Str("url", targetURL).Msg("FetchPageActivity: fetched")
	if strings.TrimSpace(html) == "" {
		return "", fmt.Errorf("empty html fetched")
	}
	return html, nil
}

func (a *Activities) AnalyzePageActivity(ctx context.Context, input map[string]any) (map[string]any, error) {
	if a.scraper == nil {
		return nil, fmt.Errorf("scraper is not initialized")
	}
	if a.registry == nil {
		return nil, fmt.Errorf("module registry is not initialized")
	}

	targetURL, _ := input["url"].(string)
	if strings.TrimSpace(targetURL) == "" {
		return nil, fmt.Errorf("url is required")
	}

	moduleName, _ := input["module"].(string)
	selectedModule := strings.ToLower(strings.TrimSpace(moduleName))
	if selectedModule == "" {
		selectedModule = "multi"
	}
	log.Info().Str("url", targetURL).Str("module", selectedModule).Msg("AnalyzePageActivity: start")
	activity.RecordHeartbeat(ctx, "analyzing")

	mod, ok := a.registry.MustGetOrDefault(selectedModule, "multi")
	if !ok {
		return nil, fmt.Errorf("module is not available: %s", selectedModule)
	}

	maxDepth := intFromAny(input["maxDepth"], 1)
	enrich := boolFromAny(input["enrich"])
	enrichLimit := intFromAny(input["enrichLimit"], 0)
	maxAge := int64FromAny(input["maxAge"], 0)

	result, err := mod.Run(ctx, a.scraper, &models.ScrapeRequest{
		Collection:  inferCollection(targetURL),
		BaseURL:     targetURL,
		MaxPages:    maxDepth,
		Enrich:      enrich,
		EnrichLimit: enrichLimit,
		MaxAge:      maxAge,
	})
	if err != nil {
		log.Error().Err(err).Str("url", targetURL).Msg("AnalyzePageActivity: module.Run error")
		return nil, err
	}

	activity.RecordHeartbeat(ctx, "analyzed")
	log.Info().Int("count", result.Count).Str("url", targetURL).Msg("AnalyzePageActivity: completed")

	b, err := json.Marshal(result)
	if err != nil {
		return nil, err
	}
	out := map[string]any{}
	if err := json.Unmarshal(b, &out); err != nil {
		return nil, err
	}
	return out, nil
}

// StoreResultActivity runs the post-processing pipeline and persists results.
func (a *Activities) StoreResultActivity(ctx context.Context, jobID string, payload map[string]any) error {
	if strings.TrimSpace(jobID) == "" {
		return fmt.Errorf("job id is required")
	}
	if payload == nil {
		return fmt.Errorf("payload is required")
	}

	activity.RecordHeartbeat(ctx, "storing")

	targetURL, _ := payload["url"].(string)
	moduleName, _ := payload["module"].(string)

	job := &pipeline.JobContext{
		JobID:     jobID,
		URL:       targetURL,
		Mode:      "scheduled",
		Module:    moduleName,
		StartedAt: time.Now(),
		Result:    payload,
		Meta:      make(map[string]string),
	}

	if a.pipeline != nil {
		if err := a.pipeline.Run(ctx, job); err != nil {
			log.Warn().Err(err).Str("job_id", jobID).Msg("StoreResultActivity: pipeline error (non-fatal)")
			// Pipeline errors are non-fatal — we still consider the result stored.
		}
	}

	// Publish NATS event for cross-plane consumers.
	if a.publisher != nil {
		metaIface := make(map[string]interface{})
		for k, v := range job.Meta {
			metaIface[k] = v
		}
		_ = a.publisher.PublishCrawlCompleted(ctx, "", targetURL, jobID, 0, metaIface)
	}

	log.Info().
		Str("job_id", jobID).
		Str("fingerprint", job.Meta["fingerprint"]).
		Str("persisted", job.Meta["persisted"]).
		Msg("StoreResultActivity: done")

	return nil
}

func inferCollection(inputURL string) string {
	if inputURL == "" {
		return ""
	}
	parts := strings.Split(strings.Trim(inputURL, "/"), "/")
	for index, part := range parts {
		if part == "produktkategori" || part == "collections" {
			if index+1 < len(parts) {
				return parts[index+1]
			}
		}
	}
	return ""
}

func intFromAny(value any, fallback int) int {
	switch typed := value.(type) {
	case int:
		return typed
	case int32:
		return int(typed)
	case int64:
		return int(typed)
	case float64:
		return int(typed)
	default:
		return fallback
	}
}

func int64FromAny(value any, fallback int64) int64 {
	switch typed := value.(type) {
	case int:
		return int64(typed)
	case int32:
		return int64(typed)
	case int64:
		return typed
	case float64:
		return int64(typed)
	default:
		return fallback
	}
}

func boolFromAny(value any) bool {
	switch typed := value.(type) {
	case bool:
		return typed
	default:
		return false
	}
}
