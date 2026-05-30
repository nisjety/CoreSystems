package scraper

import (
	"context"
	"encoding/json"
	"fmt"
	"net/url"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/rs/zerolog/log"
	"golang.org/x/sync/semaphore"
	"golang.org/x/time/rate"

	"github.com/triodelab/quarry/internal/ai"
	"github.com/triodelab/quarry/internal/cache"
	"github.com/triodelab/quarry/internal/config"
	"github.com/triodelab/quarry/internal/extractor"
	"github.com/triodelab/quarry/internal/models"
)

type Scraper struct {
	cfg           *Config
	browserPool   *BrowserPool
	extractor     *extractor.LLMExtractor
	aiClient      ai.AIClient
	aiReliability *ai.ReliabilityGate
	aiCache       *ai.ResponseCache
	rateLimiter   *rate.Limiter
	semaphore     *semaphore.Weighted
	metricsStore  *MetricsStore
	mu            sync.RWMutex
	cacheManager  *cache.Manager
}

type Config struct {
	MaxConcurrentPages int
	BrowserPoolSize    int
	UserAgent          string
	EnableStealth      bool
	ScreenshotEnabled  bool
	DefaultMaxAgeMs    int64
	AICoreBaseURL      string // Model Plane v2 base URL for OCR-backed PDF parsing
}

func New(cfg *config.Config, aiClient ai.AIClient) (*Scraper, error) {
	// Initialize Browser Pool
	browserPool, err := NewBrowserPool(cfg.BrowserPoolSize, true)
	if err != nil {
		return nil, fmt.Errorf("initialize browser pool: %w", err)
	}

	aiReliability := ai.NewReliabilityGate(aiClient)
	aiCache := ai.NewResponseCache(cfg)

	// Initialize LLM extractor with AI client for intelligent extraction
	ext, err := extractor.NewLLMExtractor(
		cfg.AzureOpenAIEndpoint,
		cfg.AzureOpenAIKey,
		cfg.AzureOpenAIModel,
		aiClient,
		aiReliability,
		aiCache,
		cfg.AIExtractionTimeoutSec,
	)
	if err != nil {
		return nil, fmt.Errorf("failed to initialize LLM extractor: %w", err)
	}

	scraperCfg := &Config{
		MaxConcurrentPages: cfg.MaxConcurrentPages,
		BrowserPoolSize:    cfg.BrowserPoolSize,
		UserAgent:          cfg.UserAgent,
		EnableStealth:      cfg.EnableStealth,
		ScreenshotEnabled:  cfg.ScreenshotEnabled,
		DefaultMaxAgeMs:    cfg.DefaultMaxAgeMs,
		AICoreBaseURL:      cfg.AICoreHTTPBaseURL,
	}

	s := &Scraper{
		cfg:           scraperCfg,
		browserPool:   browserPool,
		extractor:     ext,
		aiClient:      aiClient,
		aiReliability: aiReliability,
		aiCache:       aiCache,
		rateLimiter:   rate.NewLimiter(rate.Every(100*time.Millisecond), cfg.MaxConcurrentPages),
		semaphore:     semaphore.NewWeighted(int64(cfg.MaxConcurrentPages)),
		metricsStore:  NewMetricsStore(),
	}
	cacheManager, err := newCacheManager(cfg)
	if err != nil {
		return nil, err
	}
	s.cacheManager = cacheManager

	log.Info().Msg("Scraper initialized with Rod browser")
	return s, nil
}

func (s *Scraper) AIReliabilitySnapshot() map[string]interface{} {
	if s == nil || s.aiReliability == nil {
		return map[string]interface{}{"enabled": false}
	}
	return s.aiReliability.Snapshot()
}

func (s *Scraper) AIEfficiencySnapshot() map[string]interface{} {
	if s == nil || s.aiCache == nil {
		return map[string]interface{}{"enabled": false}
	}
	return s.aiCache.Snapshot()
}

// AIClient returns the AI client for direct use (e.g., AgentNavigate).
func (s *Scraper) AIClient() ai.AIClient {
	if s == nil {
		return nil
	}
	return s.aiClient
}

// BrowserPool returns the browser pool for use by the session manager.
func (s *Scraper) BrowserPool() *BrowserPool {
	if s == nil {
		return nil
	}
	return s.browserPool
}

// Close satisfies the io.Closer interface
// func (s *Scraper) Close() error implementation is at the end of file

func (s *Scraper) ScrapeCollection(ctx context.Context, req *models.ScrapeRequest) (*models.ScrapeResult, error) {
	startTime := time.Now()

	log.Info().
		Str("collection", req.Collection).
		Int("max_pages", req.MaxPages).
		Bool("enrich", req.Enrich).
		Msg("Starting collection scrape")

	baseURL := req.BaseURL
	if baseURL == "" {
		baseURL = fmt.Sprintf("https://skinsecret.no/produktkategori/%s", req.Collection)
	}

	var allProducts []*models.Product
	var mu sync.Mutex
	var pageWG sync.WaitGroup

	// Scrape pages concurrently
	for pageNum := 1; pageNum <= req.MaxPages; pageNum++ {
		// Wait for rate limiter
		if err := s.rateLimiter.Wait(ctx); err != nil {
			return nil, fmt.Errorf("rate limiter error: %w", err)
		}

		// Acquire semaphore
		if err := s.semaphore.Acquire(ctx, 1); err != nil {
			return nil, fmt.Errorf("semaphore acquire error: %w", err)
		}

		pageWG.Add(1)
		go func(page int) {
			defer pageWG.Done()
			defer s.semaphore.Release(1)

			url := baseURL
			if page > 1 {
				url = fmt.Sprintf("%s?page=%d", baseURL, page)
			}

			products, err := s.scrapePage(ctx, url, req.Collection, req.IncludePaths, req.ExcludePaths)
			if err != nil {
				log.Error().Err(err).Int("page", page).Msg("Failed to scrape page")
				return
			}

			if len(products) == 0 {
				log.Warn().Int("page", page).Msg("No products found, stopping pagination")
				return
			}

			// Emit page:discovered event
			emitDiscovered(req.EventSink, url, len(products))

			mu.Lock()
			allProducts = append(allProducts, products...)
			mu.Unlock()

			log.Info().
				Int("page", page).
				Int("products", len(products)).
				Int("total", len(allProducts)).
				Msg("Page scraped")
		}(pageNum)
	}

	// Wait for all goroutines to complete
	pageWG.Wait()

	// Deduplicate by URL
	uniqueProducts := s.deduplicateProducts(allProducts)

	// Prompt-guided link filtering: score URLs by keyword relevance to prompt
	if req.Prompt != "" && len(uniqueProducts) > 0 {
		uniqueProducts = scoreAndSortByPrompt(uniqueProducts, req.Prompt)
		log.Info().
			Str("prompt", req.Prompt).
			Int("candidates", len(uniqueProducts)).
			Msg("URLs sorted by prompt relevance")
	}

	// Enrich products if requested
	var enrichedProducts []*models.Product
	var enrichmentWG sync.WaitGroup
	jsonExtractionCount := 0
	emptyProducts := 0

	if req.Enrich && len(uniqueProducts) > 0 {
		limit := req.EnrichLimit
		if limit > len(uniqueProducts) {
			limit = len(uniqueProducts)
		}

		enrichedProducts = make([]*models.Product, limit)

		// Map to store indices to maintain order
		for i := 0; i < limit; i++ {
			enrichmentWG.Add(1)

			// Wait for rate limiter
			if err := s.rateLimiter.Wait(ctx); err != nil {
				enrichmentWG.Done()
				break
			}

			// Acquire semaphore
			if err := s.semaphore.Acquire(ctx, 1); err != nil {
				enrichmentWG.Done()
				break
			}

			go func(index int, p *models.Product) {
				defer s.semaphore.Release(1)
				defer enrichmentWG.Done()

				enriched, err := s.enrichPage(ctx, p, req.EventSink, req.Schema, req.Prompt)
				if err != nil {
					log.Error().Err(err).Str("url", p.URL).Msg("Failed to enrich page")
					enrichedProducts[index] = p
					return
				}

				enrichedProducts[index] = enriched
			}(i, uniqueProducts[i])
		}

		// Wait for current batch to finish
		enrichmentWG.Wait()

		// Add remaining products without enrichment
		if limit < len(uniqueProducts) {
			enrichedProducts = append(enrichedProducts, uniqueProducts[limit:]...)
		}
	} else {
		enrichedProducts = uniqueProducts
	}

	// Calculate counts from results
	for _, p := range enrichedProducts {
		if p != nil && (p.Content != "" || p.Description != "") {
			jsonExtractionCount++
		} else {
			emptyProducts++
		}
	}

	duration := time.Since(startTime)

	// Record metrics
	metrics := &models.EnrichmentMetrics{
		Timestamp:             time.Now(),
		Collection:            req.Collection,
		TotalProducts:         len(enrichedProducts),
		EnrichedCount:         jsonExtractionCount,
		JSONExtractionCount:   jsonExtractionCount,
		MarkdownFallbackCount: 0,
		EmptyProducts:         emptyProducts,
		SuccessRate:           0,
		JSONSuccessRate:       0,
		Duration:              duration,
	}
	if len(enrichedProducts) > 0 {
		metrics.SuccessRate = float64(jsonExtractionCount) / float64(len(enrichedProducts)) * 100
		metrics.JSONSuccessRate = float64(jsonExtractionCount) / float64(len(enrichedProducts)) * 100
	}

	s.metricsStore.AddMetric(metrics)

	log.Info().
		Str("collection", req.Collection).
		Int("total_products", len(enrichedProducts)).
		Int("enriched", jsonExtractionCount).
		Float64("success_rate", metrics.SuccessRate).
		Dur("duration", duration).
		Msg("Collection scrape completed")

	return &models.ScrapeResult{
		Count:    len(enrichedProducts),
		Products: enrichedProducts,
		Metrics:  metrics,
	}, nil
}

// scoreAndSortByPrompt scores discovered URLs by keyword relevance to the user prompt.
// URLs whose path segments match more keywords are ranked higher.
func scoreAndSortByPrompt(products []*models.Product, prompt string) []*models.Product {
	// Extract keywords from prompt (split on whitespace + common delimiters, lowercase, skip short words)
	raw := strings.FieldsFunc(strings.ToLower(prompt), func(r rune) bool {
		return r == ' ' || r == ',' || r == '.' || r == ';' || r == ':' || r == '!' || r == '?'
	})
	var keywords []string
	stopWords := map[string]bool{
		"the": true, "a": true, "an": true, "and": true, "or": true, "of": true,
		"to": true, "in": true, "for": true, "is": true, "on": true, "at": true,
		"by": true, "with": true, "from": true, "all": true, "me": true,
		"find": true, "get": true, "show": true, "list": true, "i": true,
		"want": true, "need": true, "about": true, "that": true, "this": true,
	}
	for _, w := range raw {
		if len(w) >= 3 && !stopWords[w] {
			keywords = append(keywords, w)
		}
	}
	if len(keywords) == 0 {
		return products
	}

	type scored struct {
		product *models.Product
		score   int
	}
	scored_list := make([]scored, len(products))
	for i, p := range products {
		// Score against URL path segments
		parsed, err := url.Parse(p.URL)
		pathLower := ""
		if err == nil {
			pathLower = strings.ToLower(parsed.Path)
		} else {
			pathLower = strings.ToLower(p.URL)
		}

		score := 0
		for _, kw := range keywords {
			if strings.Contains(pathLower, kw) {
				score += 2
			}
		}
		// Also score against the product name/title if available
		if p.Name != "" {
			nameLower := strings.ToLower(p.Name)
			for _, kw := range keywords {
				if strings.Contains(nameLower, kw) {
					score++
				}
			}
		}
		scored_list[i] = scored{product: p, score: score}
	}

	// Stable sort: highest score first, preserve original order for ties
	sort.SliceStable(scored_list, func(i, j int) bool {
		return scored_list[i].score > scored_list[j].score
	})

	result := make([]*models.Product, len(products))
	for i, s := range scored_list {
		result[i] = s.product
	}
	return result
}

func (s *Scraper) scrapePage(ctx context.Context, pageURL, collection string, includePaths, excludePaths []string) ([]*models.Product, error) {
	log.Debug().Str("url", pageURL).Msg("Getting page from pool")
	page, cleanup, err := s.browserPool.GetPage(ctx)
	if err != nil {
		return nil, fmt.Errorf("failed to get page from pool: %w", err)
	}
	log.Debug().Str("url", pageURL).Msg("Got page, navigating")
	defer cleanup()

	// Navigate to page
	if err := page.Navigate(pageURL); err != nil {
		return nil, fmt.Errorf("navigation failed: %w", err)
	}
	log.Debug().Str("url", pageURL).Msg("Navigated, waiting for load")

	// Wait for page load
	if err := page.WaitLoad(); err != nil {
		return nil, fmt.Errorf("wait load failed: %w", err)
	}
	log.Debug().Str("url", pageURL).Msg("Page loaded, getting HTML")

	// Extract product links from page
	html, err := page.HTML()
	if err != nil {
		return nil, fmt.Errorf("failed to get HTML: %w", err)
	}

	products := extractProductLinks(html, pageURL, collection, includePaths, excludePaths)

	return products, nil
}

// emitDiscovered sends a page:discovered event if a sink is available.
func emitDiscovered(sink models.PageEventSink, url string, count int) {
	if sink != nil {
		sink.OnPageDiscovered(url, count)
	}
}

func (s *Scraper) enrichPage(ctx context.Context, product *models.Product, sink models.PageEventSink, schema string, prompt string) (*models.Product, error) {
	page, cleanup, err := s.browserPool.GetPage(ctx)
	if err != nil {
		return product, fmt.Errorf("failed to get page from pool: %w", err)
	}
	defer cleanup()

	// Navigate to page
	if err := page.Navigate(product.URL); err != nil {
		return product, fmt.Errorf("navigation failed: %w", err)
	}

	if err := page.WaitLoad(); err != nil {
		return product, fmt.Errorf("wait load failed: %w", err)
	}

	// Get HTML content
	html, err := page.HTML()
	if err != nil {
		return product, fmt.Errorf("failed to get HTML: %w", err)
	}

	// Screenshot if enabled
	if s.cfg.ScreenshotEnabled {
		_, _ = page.Screenshot(true, nil)
	}

	// Smart extraction: classify page and extract with the right method
	extraction, err := s.extractor.ExtractSmart(ctx, html, product.URL, schema, prompt)
	if err != nil {
		return product, fmt.Errorf("smart extraction failed: %w", err)
	}

	// Emit page:classified event
	if sink != nil {
		sink.OnPageClassified(product.URL, extraction.PageType, extraction.Confidence)
	}

	// Always set universal fields
	product.PageType = extraction.PageType
	product.Title = extraction.Title
	product.Content = extraction.Content

	// Map type-specific structured data
	switch extraction.PageType {
	case models.PageTypeProduct:
		// Product pages: fill existing product fields for backwards compatibility
		if details, ok := extraction.Structured.(*models.ProductDetailSchema); ok && details != nil {
			product.Name = details.Name
			product.Brand = details.Brand
			product.CurrentPrice = details.CurrentPrice
			product.OriginalPrice = details.OriginalPrice
			product.OnSale = details.OnSale
			product.InStock = details.InStock
			product.Available = details.Available
			product.Description = details.Description
			product.UseCase = details.UseCase
			product.Ingredients = details.Ingredients
		}
	case models.PageTypeHomepage, models.PageTypeAbout, models.PageTypeTeam:
		if data, ok := extraction.Structured.(*models.CompanyIntelligenceSchema); ok && data != nil {
			product.Name = data.CompanyName
			product.Description = data.Description
			product.Metadata = structToMap(data)
		}
	case models.PageTypeArticle:
		if data, ok := extraction.Structured.(*models.ArticleSchema); ok && data != nil {
			product.Name = data.Title
			product.Description = data.Summary
			product.Metadata = structToMap(data)
		}
	case models.PageTypeContact:
		if data, ok := extraction.Structured.(*models.ContactPageSchema); ok && data != nil {
			product.Name = data.CompanyName
			product.Metadata = structToMap(data)
		}
	case models.PageTypeServices:
		if data, ok := extraction.Structured.(*models.ServicePageSchema); ok && data != nil {
			product.Name = data.CompanyName
			product.Metadata = structToMap(data)
		}
	default:
		if data, ok := extraction.Structured.(*models.GenericPageSchema); ok && data != nil {
			product.Name = data.Title
			product.Description = data.MainText
			if len(product.Description) > 500 {
				product.Description = product.Description[:500]
			}
			product.Metadata = structToMap(data)
		}
	}

	// Emit page:enriched event
	if sink != nil {
		sink.OnPageEnriched(product.URL, product.PageType, product.Title)
	}

	return product, nil
}

// structToMap converts a struct to map[string]interface{} via JSON round-trip.
func structToMap(v interface{}) map[string]interface{} {
	data, err := json.Marshal(v)
	if err != nil {
		return nil
	}
	var m map[string]interface{}
	if err := json.Unmarshal(data, &m); err != nil {
		return nil
	}
	return m
}

func (s *Scraper) deduplicateProducts(products []*models.Product) []*models.Product {
	seen := make(map[string]bool)
	unique := make([]*models.Product, 0, len(products))

	for _, p := range products {
		if !seen[p.URL] {
			seen[p.URL] = true
			unique = append(unique, p)
		}
	}

	return unique
}

func (s *Scraper) Close() error {
	s.mu.Lock()
	defer s.mu.Unlock()

	if s.aiReliability != nil {
		s.aiReliability.Close()
	}

	if s.aiCache != nil {
		if err := s.aiCache.Close(); err != nil {
			return fmt.Errorf("failed to close ai cache: %w", err)
		}
	}

	if s.browserPool != nil {
		if err := s.browserPool.Close(); err != nil {
			return fmt.Errorf("failed to close browser pool: %w", err)
		}
	}

	if s.cacheManager != nil {
		if err := s.cacheManager.Close(); err != nil {
			return fmt.Errorf("failed to close cache manager: %w", err)
		}
	}

	return nil
}
