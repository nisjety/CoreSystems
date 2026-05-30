package models

import "time"

// Page type constants for the smart extraction pipeline.
const (
	PageTypeHomepage = "homepage"
	PageTypeAbout    = "about"
	PageTypeProduct  = "product"
	PageTypeArticle  = "article"
	PageTypeContact  = "contact"
	PageTypeServices = "services"
	PageTypeTeam     = "team"
	PageTypePricing  = "pricing"
	PageTypeGeneric  = "generic"
	PageTypeCustom   = "custom"
)

type Product struct {
	SKU           string `json:"sku"`
	URL           string `json:"url"`
	Name          string `json:"name,omitempty"`
	Brand         string `json:"brand,omitempty"`
	Category      string `json:"category"`
	CurrentPrice  int    `json:"current_price,omitempty"`
	OriginalPrice int    `json:"original_price,omitempty"`
	OnSale        bool   `json:"on_sale"`
	InStock       bool   `json:"in_stock"`
	URLValid      bool   `json:"url_valid"`
	Available     int    `json:"available"`
	Description   string `json:"description,omitempty"`
	UseCase       string `json:"use_case,omitempty"`
	Ingredients   string `json:"ingredients,omitempty"`

	// Smart extraction fields (Phase 2)
	PageType string                 `json:"page_type,omitempty"`
	Title    string                 `json:"title,omitempty"`
	Content  string                 `json:"content,omitempty"`
	Metadata map[string]interface{} `json:"metadata,omitempty"`
}

// PageEventSink receives real-time page-level events during crawl/enrichment.
// Implementations must be safe for concurrent use.
type PageEventSink interface {
	OnPageDiscovered(url string, count int)
	OnPageClassified(url string, pageType string, confidence float64)
	OnPageEnriched(url string, pageType string, title string)
}

type ScrapeRequest struct {
	BaseURL        string                 `json:"base_url"`
	Collection     string                 `json:"collection"`
	MaxPages       int                    `json:"max_pages"`
	Enrich         bool                   `json:"enrich"`
	EnrichLimit    int                    `json:"enrich_limit"`
	Output         string                 `json:"output,omitempty"`
	MaxAge         int64                  `json:"max_age"`
	ChangeTracking *ChangeTrackingRequest `json:"changeTracking,omitempty"` // Change tracking options
	EventSink      PageEventSink          `json:"-"`                        // Optional real-time event sink

	// Smart crawl fields
	IncludePaths []string `json:"includePaths,omitempty"` // Glob patterns for URLs to include
	ExcludePaths []string `json:"excludePaths,omitempty"` // Glob patterns for URLs to exclude
	Schema       string   `json:"schema,omitempty"`       // User-defined JSON schema for extraction
	Prompt       string   `json:"prompt,omitempty"`       // Prompt to guide extraction and link filtering
}

type ScrapeResult struct {
	Count    int                `json:"count"`
	Products []*Product         `json:"products"`
	Metrics  *EnrichmentMetrics `json:"metrics,omitempty"`
}

type EnrichmentMetrics struct {
	Timestamp             time.Time     `json:"timestamp"`
	Collection            string        `json:"collection"`
	TotalProducts         int           `json:"total_products"`
	EnrichedCount         int           `json:"enriched_count"`
	JSONExtractionCount   int           `json:"json_extraction_count"`
	MarkdownFallbackCount int           `json:"markdown_fallback_count"`
	EmptyProducts         int           `json:"empty_products"`
	SuccessRate           float64       `json:"success_rate"`
	JSONSuccessRate       float64       `json:"json_success_rate"`
	Duration              time.Duration `json:"duration"`
}

type ProductDetailSchema struct {
	Name          string `json:"name"`
	Brand         string `json:"brand"`
	CurrentPrice  int    `json:"current_price"`
	OriginalPrice int    `json:"original_price"`
	OnSale        bool   `json:"on_sale"`
	InStock       bool   `json:"in_stock"`
	Available     int    `json:"available"`
	Description   string `json:"description"`
	UseCase       string `json:"use_case"`
	Ingredients   string `json:"ingredients"`
}

// SmartExtraction is the unified result from the purpose-adaptive extraction pipeline.
type SmartExtraction struct {
	PageType   string      `json:"page_type"`
	Title      string      `json:"title"`
	Content    string      `json:"content"`
	Structured interface{} `json:"structured"`
	Confidence float64     `json:"confidence"`
	Method     string      `json:"method"` // "ai" or "heuristic"
}

// ArticleSchema holds structured data extracted from blog posts and articles.
type ArticleSchema struct {
	Title         string   `json:"title"`
	Author        string   `json:"author"`
	PublishedDate string   `json:"published_date"`
	Summary       string   `json:"summary"`
	Body          string   `json:"body"`
	Tags          []string `json:"tags"`
	ReadingTime   string   `json:"reading_time"`
}

// ContactPageSchema holds structured data extracted from contact pages.
type ContactPageSchema struct {
	Emails      []string `json:"emails"`
	Phones      []string `json:"phones"`
	Addresses   []string `json:"addresses"`
	OfficeHours string   `json:"office_hours"`
	SocialLinks []string `json:"social_links"`
	CompanyName string   `json:"company_name"`
	MapURL      string   `json:"map_url"`
}

// ServicePageSchema holds structured data extracted from services/offerings pages.
type ServicePageSchema struct {
	CompanyName string           `json:"company_name"`
	Services    []ServiceDetail  `json:"services"`
}

// ServiceDetail describes a single service offering.
type ServiceDetail struct {
	Name        string `json:"name"`
	Description string `json:"description"`
	PricingHint string `json:"pricing_hint"`
}

// GenericPageSchema holds structured data for pages that don't match a specific type.
type GenericPageSchema struct {
	Title    string   `json:"title"`
	Headings []string `json:"headings"`
	MainText string   `json:"main_text"`
	KeyLinks []string `json:"key_links"`
}

// CompanyIntelligenceSchema holds structured company information extracted by AI.
type CompanyIntelligenceSchema struct {
	CompanyName string   `json:"company_name"`
	Description string   `json:"description"`
	Industry    string   `json:"industry"`
	Services    []string `json:"services"`
	Products    []string `json:"products"`
	Location    string   `json:"location"`
	Founded     string   `json:"founded"`
	TeamSize    string   `json:"team_size"`
	Email       string   `json:"email"`
	Phone       string   `json:"phone"`
	Website     string   `json:"website"`
	SocialLinks []string `json:"social_links"`
	Tagline     string   `json:"tagline"`
	Mission     string   `json:"mission"`
	KeyPeople   []string `json:"key_people"`
}
