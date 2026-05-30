package extractor

import (
	"context"
	"encoding/json"
	"fmt"
	"regexp"
	"strconv"
	"strings"
	"time"

	"github.com/PuerkitoBio/goquery"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/quarry/internal/ai"
	"github.com/triodelab/quarry/internal/models"
)

type LLMExtractor struct {
	endpoint     string
	key          string
	model        string
	aiClient     ai.AIClient
	aiGate       *ai.ReliabilityGate
	aiCache      *ai.ResponseCache
	aiTimeoutSec int
}

func NewLLMExtractor(endpoint, key, model string, aiClient ai.AIClient, aiGate *ai.ReliabilityGate, aiCache *ai.ResponseCache, aiTimeoutSec int) (*LLMExtractor, error) {
	return &LLMExtractor{
		endpoint:     endpoint,
		key:          key,
		model:        model,
		aiClient:     aiClient,
		aiGate:       aiGate,
		aiCache:      aiCache,
		aiTimeoutSec: aiTimeoutSec,
	}, nil
}

func (e *LLMExtractor) ExtractProductDetails(ctx context.Context, html string) (*models.ProductDetailSchema, error) {
	// Try AI extraction first if client is available
	if e.aiClient != nil {
		result, err := e.extractWithAI(ctx, html)
		if err == nil {
			log.Debug().Str("method", "ai").Msg("extraction successful via ai-core")
			return result, nil
		}
		log.Warn().Err(err).Msg("ai extraction failed, falling back to heuristic")
	}

	// Fallback to heuristic extraction
	log.Debug().Str("method", "heuristic").Msg("using heuristic extraction")
	return e.extractHeuristic(html)
}

func (e *LLMExtractor) extractWithAI(ctx context.Context, html string) (*models.ProductDetailSchema, error) {
	if e.aiGate != nil {
		if err := e.aiGate.BeforeAttempt(); err != nil {
			return nil, err
		}
	}

	start := time.Now()
	success := false
	defer func() {
		if e.aiGate != nil {
			e.aiGate.AfterAttempt(success, time.Since(start))
		}
	}()

	// Use snake_case keys matching Go's ProductDetailSchema json tags
	schema := `{
		"type": "object",
		"properties": {
			"name": {"type": "string"},
			"brand": {"type": "string"},
			"current_price": {"type": "number"},
			"original_price": {"type": "number"},
			"on_sale": {"type": "boolean"},
			"in_stock": {"type": "boolean"},
			"description": {"type": "string"},
			"use_case": {"type": "string"},
			"ingredients": {"type": "string"}
		}
	}`

	if e.aiCache != nil {
		if cached, ok := e.aiCache.GetExtract(ctx, html, schema); ok {
			var decoded models.ProductDetailSchema
			if err := json.Unmarshal([]byte(cached), &decoded); err == nil {
				success = true
				return &decoded, nil
			}
			log.Warn().Msg("failed to decode cached product payload")
		}
	}

	extractCtx, cancel := context.WithTimeout(ctx, time.Duration(e.aiTimeoutSec)*time.Second)
	defer cancel()

	// Dereference JSON Schema $refs so the model receives the fully-expanded schema.
	resolvedSchema := schema
	if derefed, err := ai.DereferenceSchema(schema); err == nil {
		resolvedSchema = derefed
	}

	resp, err := e.aiClient.ExtractData(extractCtx, &ai.ExtractRequest{
		HTML:   html,
		Schema: resolvedSchema,
	})
	if err != nil {
		return nil, err
	}

	var decoded models.ProductDetailSchema
	err = json.Unmarshal([]byte(resp.Data), &decoded)
	if err != nil {
		return nil, err
	}

	if e.aiCache != nil {
		e.aiCache.SetExtract(ctx, html, schema, resp.Data)
	}

	success = true
	return &decoded, nil
}

func (e *LLMExtractor) extractHeuristic(html string) (*models.ProductDetailSchema, error) {
	doc, err := goquery.NewDocumentFromReader(strings.NewReader(html))
	if err != nil {
		return &models.ProductDetailSchema{}, nil
	}

	name := strings.TrimSpace(doc.Find("h1").First().Text())
	brand := strings.TrimSpace(doc.Find("[itemprop='brand'], .brand, a[href*='brand']").First().Text())
	description := strings.TrimSpace(doc.Find("[itemprop='description'], .product-description, .description").First().Text())
	ingredients := strings.TrimSpace(doc.Find(".ingredients, [data-ingredients], #ingredients").First().Text())
	useCase := strings.TrimSpace(doc.Find(".usage, .how-to-use, #use, #anvendelse").First().Text())

	if len(description) > 500 {
		description = description[:500]
	}
	if len(useCase) > 300 {
		useCase = useCase[:300]
	}
	if len(ingredients) > 2000 {
		ingredients = ingredients[:2000]
	}

	priceText := strings.TrimSpace(doc.Find(".price, [itemprop='price'], .product-price").First().Text())
	currentPrice := extractPrice(priceText)
	originalText := strings.TrimSpace(doc.Find(".old-price, .compare-price, .was-price").First().Text())
	originalPrice := extractPrice(originalText)

	inStockText := strings.ToLower(doc.Text())
	inStock := strings.Contains(inStockText, "på lager") || strings.Contains(inStockText, "in stock")
	onSale := originalPrice > 0 && currentPrice > 0 && originalPrice > currentPrice

	return &models.ProductDetailSchema{
		Name:          name,
		Brand:         brand,
		CurrentPrice:  currentPrice,
		OriginalPrice: originalPrice,
		OnSale:        onSale,
		InStock:       inStock,
		Available:     0,
		Description:   description,
		UseCase:       useCase,
		Ingredients:   ingredients,
	}, nil
}

// ExtractCompanyIntelligence extracts company-level information from HTML.
// This is the primary extraction method for the Agecia company intelligence use case.
func (e *LLMExtractor) ExtractCompanyIntelligence(ctx context.Context, html string) (*models.CompanyIntelligenceSchema, error) {
	// Try AI extraction first if client is available
	if e.aiClient != nil {
		result, err := e.extractCompanyWithAI(ctx, html)
		if err == nil {
			log.Debug().Str("method", "ai").Msg("company extraction successful via ai-core")
			return result, nil
		}
		log.Warn().Err(err).Msg("ai company extraction failed, falling back to heuristic")
	}

	// Fallback to company heuristic extraction
	log.Debug().Str("method", "heuristic").Msg("using company heuristic extraction")
	return e.extractCompanyHeuristic(html)
}

func (e *LLMExtractor) extractCompanyWithAI(ctx context.Context, html string) (*models.CompanyIntelligenceSchema, error) {
	if e.aiGate != nil {
		if err := e.aiGate.BeforeAttempt(); err != nil {
			return nil, err
		}
	}

	start := time.Now()
	success := false
	defer func() {
		if e.aiGate != nil {
			e.aiGate.AfterAttempt(success, time.Since(start))
		}
	}()

	schema := `{
		"type": "object",
		"properties": {
			"company_name": {"type": "string"},
			"description": {"type": "string"},
			"industry": {"type": "string"},
			"services": {"type": "array", "items": {"type": "string"}},
			"products": {"type": "array", "items": {"type": "string"}},
			"location": {"type": "string"},
			"founded": {"type": "string"},
			"team_size": {"type": "string"},
			"email": {"type": "string"},
			"phone": {"type": "string"},
			"website": {"type": "string"},
			"social_links": {"type": "array", "items": {"type": "string"}},
			"tagline": {"type": "string"},
			"mission": {"type": "string"},
			"key_people": {"type": "array", "items": {"type": "string"}}
		}
	}`

	if e.aiCache != nil {
		if cached, ok := e.aiCache.GetExtract(ctx, html, schema); ok {
			var result models.CompanyIntelligenceSchema
			if err := json.Unmarshal([]byte(cached), &result); err == nil {
				success = true
				return &result, nil
			}
		}
	}

	extractCtx, cancel := context.WithTimeout(ctx, time.Duration(e.aiTimeoutSec)*time.Second)
	defer cancel()

	resp, err := e.aiClient.ExtractData(extractCtx, &ai.ExtractRequest{
		HTML:   html,
		Schema: schema,
	})
	if err != nil {
		return nil, err
	}

	var result models.CompanyIntelligenceSchema
	if err := json.Unmarshal([]byte(resp.Data), &result); err != nil {
		return nil, err
	}

	if e.aiCache != nil {
		e.aiCache.SetExtract(ctx, html, schema, resp.Data)
	}

	success = true
	return &result, nil
}

// extractCompanyHeuristic extracts company information using HTML meta tags,
// JSON-LD structured data, Open Graph tags, and common page selectors.
func (e *LLMExtractor) extractCompanyHeuristic(html string) (*models.CompanyIntelligenceSchema, error) {
	doc, err := goquery.NewDocumentFromReader(strings.NewReader(html))
	if err != nil {
		return &models.CompanyIntelligenceSchema{}, nil
	}

	result := &models.CompanyIntelligenceSchema{}

	// Company name: og:site_name > og:title > title > h1
	result.CompanyName = metaContent(doc, "og:site_name")
	if result.CompanyName == "" {
		result.CompanyName = metaContent(doc, "og:title")
	}
	if result.CompanyName == "" {
		result.CompanyName = strings.TrimSpace(doc.Find("title").First().Text())
	}
	if result.CompanyName == "" {
		result.CompanyName = strings.TrimSpace(doc.Find("h1").First().Text())
	}

	// Description: og:description > meta description
	result.Description = metaContent(doc, "og:description")
	if result.Description == "" {
		result.Description = metaNameContent(doc, "description")
	}

	// Website URL from og:url or canonical
	result.Website = metaContent(doc, "og:url")
	if result.Website == "" {
		result.Website, _ = doc.Find("link[rel='canonical']").Attr("href")
	}

	// Email and phone from links
	doc.Find("a[href^='mailto:']").Each(func(_ int, s *goquery.Selection) {
		href, _ := s.Attr("href")
		email := strings.TrimPrefix(href, "mailto:")
		if result.Email == "" && email != "" {
			result.Email = email
		}
	})
	doc.Find("a[href^='tel:']").Each(func(_ int, s *goquery.Selection) {
		href, _ := s.Attr("href")
		phone := strings.TrimPrefix(href, "tel:")
		if result.Phone == "" && phone != "" {
			result.Phone = phone
		}
	})

	// Social links
	socialDomains := []string{"linkedin.com", "twitter.com", "x.com", "facebook.com", "instagram.com", "github.com", "youtube.com"}
	doc.Find("a[href]").Each(func(_ int, s *goquery.Selection) {
		href, _ := s.Attr("href")
		for _, domain := range socialDomains {
			if strings.Contains(href, domain) {
				result.SocialLinks = append(result.SocialLinks, href)
				break
			}
		}
	})

	// JSON-LD structured data
	doc.Find("script[type='application/ld+json']").Each(func(_ int, s *goquery.Selection) {
		var ld map[string]interface{}
		if err := json.Unmarshal([]byte(s.Text()), &ld); err != nil {
			return
		}
		ldType, _ := ld["@type"].(string)
		if ldType == "Organization" || ldType == "LocalBusiness" || ldType == "Corporation" {
			if name, ok := ld["name"].(string); ok && result.CompanyName == "" {
				result.CompanyName = name
			}
			if desc, ok := ld["description"].(string); ok && result.Description == "" {
				result.Description = desc
			}
			if email, ok := ld["email"].(string); ok && result.Email == "" {
				result.Email = email
			}
			if phone, ok := ld["telephone"].(string); ok && result.Phone == "" {
				result.Phone = phone
			}
			if addr, ok := ld["address"].(map[string]interface{}); ok {
				var parts []string
				for _, key := range []string{"streetAddress", "addressLocality", "addressRegion", "addressCountry"} {
					if v, ok := addr[key].(string); ok && v != "" {
						parts = append(parts, v)
					}
				}
				if result.Location == "" && len(parts) > 0 {
					result.Location = strings.Join(parts, ", ")
				}
			}
		}
	})

	return result, nil
}

func metaContent(doc *goquery.Document, property string) string {
	val := ""
	doc.Find("meta[property='" + property + "']").Each(func(_ int, s *goquery.Selection) {
		if content, exists := s.Attr("content"); exists && val == "" {
			val = strings.TrimSpace(content)
		}
	})
	return val
}

func metaNameContent(doc *goquery.Document, name string) string {
	val := ""
	doc.Find("meta[name='" + name + "']").Each(func(_ int, s *goquery.Selection) {
		if content, exists := s.Attr("content"); exists && val == "" {
			val = strings.TrimSpace(content)
		}
	})
	return val
}

// ExtractSmart is the purpose-adaptive extraction router.
// It classifies the page, then routes to the correct type-specific extractor.
//
// Behaviour modes:
//   - userSchema set: skip classification, extract using the user's schema directly ("custom" mode)
//   - userPrompt set (no schema): classify normally, but pass prompt as AI context for guided extraction
//   - neither: fully automatic classification + type-specific extraction
func (e *LLMExtractor) ExtractSmart(ctx context.Context, html string, pageURL string, userSchema string, userPrompt string) (*models.SmartExtraction, error) {
	doc, err := goquery.NewDocumentFromReader(strings.NewReader(html))
	if err != nil {
		return nil, err
	}

	// Extract page title (universal)
	title := strings.TrimSpace(doc.Find("title").First().Text())
	if title == "" {
		title = strings.TrimSpace(doc.Find("h1").First().Text())
	}

	// Extract main text content (universal, truncated)
	content := extractMainContent(doc)

	// === Mode 1: User-defined schema — skip classification, extract exactly what user wants ===
	if strings.TrimSpace(userSchema) != "" {
		result := &models.SmartExtraction{
			PageType:   models.PageTypeCustom,
			Title:      title,
			Content:    content,
			Confidence: 1.0,
		}
		customData, err := e.ExtractWithSchema(ctx, html, userSchema)
		if err == nil {
			result.Structured = customData
			result.Method = "ai"
		} else {
			log.Warn().Err(err).Str("url", pageURL).Msg("custom schema extraction failed")
			result.Method = "heuristic"
		}
		return result, nil
	}

	// === Mode 2 & 3: Auto-classify ===
	pageType, confidence := ClassifyPage(pageURL, doc)

	result := &models.SmartExtraction{
		PageType:   pageType,
		Title:      title,
		Content:    content,
		Confidence: confidence,
	}

	log.Info().
		Str("page_type", pageType).
		Float64("confidence", confidence).
		Str("url", pageURL).
		Bool("has_prompt", userPrompt != "").
		Msg("page classified")

	// === Mode 2: Prompt-guided — auto-classify but pass prompt context to AI ===
	if strings.TrimSpace(userPrompt) != "" {
		guidedData, err := e.ExtractWithPrompt(ctx, html, pageType, userPrompt)
		if err == nil {
			result.Structured = guidedData
			result.Method = "ai"
		} else {
			log.Warn().Err(err).Str("url", pageURL).Msg("prompt-guided extraction failed, falling back to auto")
			// Fall through to normal auto extraction below
			goto AUTO_EXTRACT
		}
		return result, nil
	}

AUTO_EXTRACT:
	// === Mode 3: Fully automatic — route to type-specific extractor ===
	switch pageType {
	case models.PageTypeHomepage, models.PageTypeAbout, models.PageTypeTeam:
		companyData, err := e.ExtractCompanyIntelligence(ctx, html)
		if err == nil {
			result.Structured = companyData
			result.Method = "ai"
		} else {
			result.Method = "heuristic"
		}
	case models.PageTypeProduct:
		productData, err := e.ExtractProductDetails(ctx, html)
		if err == nil {
			result.Structured = productData
			result.Method = "ai"
		} else {
			result.Method = "heuristic"
		}
	case models.PageTypeArticle:
		articleData, err := e.ExtractArticle(ctx, html)
		if err == nil {
			result.Structured = articleData
			result.Method = "ai"
		} else {
			result.Method = "heuristic"
		}
	case models.PageTypeContact:
		contactData, err := e.ExtractContactInfo(ctx, html)
		if err == nil {
			result.Structured = contactData
			result.Method = "ai"
		} else {
			result.Method = "heuristic"
		}
	case models.PageTypeServices:
		serviceData, err := e.ExtractServices(ctx, html)
		if err == nil {
			result.Structured = serviceData
			result.Method = "ai"
		} else {
			result.Method = "heuristic"
		}
	default:
		genericData, err := e.ExtractGenericContent(ctx, html)
		if err == nil {
			result.Structured = genericData
			result.Method = "ai"
		} else {
			result.Method = "heuristic"
		}
	}

	log.Info().
		Str("page_type", pageType).
		Str("method", result.Method).
		Str("url", pageURL).
		Msg("smart extraction complete")

	return result, nil
}

// ExtractWithSchema extracts data using a user-provided JSON schema.
// This sends the schema directly to ai-core and returns the raw structured result.
// The model tier is automatically selected based on schema complexity and HTML size.
func (e *LLMExtractor) ExtractWithSchema(ctx context.Context, html string, schema string) (map[string]interface{}, error) {
	if e.aiClient == nil {
		return nil, fmt.Errorf("ai client is not available")
	}

	tier := ai.RouteModel(schema, len(html))

	extractCtx, cancel := context.WithTimeout(ctx, time.Duration(e.aiTimeoutSec)*time.Second)
	defer cancel()

	resp, err := e.aiClient.ExtractData(extractCtx, &ai.ExtractRequest{
		HTML:      html,
		Schema:    schema,
		ModelHint: tier,
	})
	if err != nil {
		return nil, err
	}

	var result map[string]interface{}
	if err := json.Unmarshal([]byte(resp.Data), &result); err != nil {
		return nil, err
	}
	return result, nil
}

// ExtractWithPrompt extracts data using auto-detected page type but guided by a user prompt.
// The prompt is prepended as context to the AI extraction call.
func (e *LLMExtractor) ExtractWithPrompt(ctx context.Context, html string, pageType string, prompt string) (map[string]interface{}, error) {
	if e.aiClient == nil {
		return nil, fmt.Errorf("ai client is not available")
	}

	// Build a schema that asks AI to extract based on the prompt + page type context
	schema := `{
		"type": "object",
		"description": "Extract structured data from this ` + pageType + ` page. User instruction: ` + strings.ReplaceAll(prompt, `"`, `'`) + `",
		"properties": {
			"title": {"type": "string"},
			"summary": {"type": "string"},
			"key_data": {"type": "object", "description": "The main structured data requested by the user"},
			"relevant_links": {"type": "array", "items": {"type": "string"}}
		}
	}`

	extractCtx, cancel := context.WithTimeout(ctx, time.Duration(e.aiTimeoutSec)*time.Second)
	defer cancel()

	tier := ai.RouteModel(schema, len(html))

	resp, err := e.aiClient.ExtractData(extractCtx, &ai.ExtractRequest{
		HTML:      html,
		Schema:    schema,
		ModelHint: tier,
	})
	if err != nil {
		return nil, err
	}

	var result map[string]interface{}
	if err := json.Unmarshal([]byte(resp.Data), &result); err != nil {
		return nil, err
	}
	return result, nil
}

// ExtractArticle extracts article/blog content from HTML.
func (e *LLMExtractor) ExtractArticle(ctx context.Context, html string) (*models.ArticleSchema, error) {
	if e.aiClient != nil {
		result, err := e.extractArticleWithAI(ctx, html)
		if err == nil {
			log.Debug().Str("method", "ai").Msg("article extraction successful via ai-core")
			return result, nil
		}
		log.Warn().Err(err).Msg("ai article extraction failed, falling back to heuristic")
	}
	return e.extractArticleHeuristic(html)
}

func (e *LLMExtractor) extractArticleWithAI(ctx context.Context, html string) (*models.ArticleSchema, error) {
	schema := `{
		"type": "object",
		"properties": {
			"title": {"type": "string"},
			"author": {"type": "string"},
			"published_date": {"type": "string"},
			"summary": {"type": "string"},
			"body": {"type": "string"},
			"tags": {"type": "array", "items": {"type": "string"}},
			"reading_time": {"type": "string"}
		}
	}`

	extractCtx, cancel := context.WithTimeout(ctx, time.Duration(e.aiTimeoutSec)*time.Second)
	defer cancel()

	resp, err := e.aiClient.ExtractData(extractCtx, &ai.ExtractRequest{
		HTML:   html,
		Schema: schema,
	})
	if err != nil {
		return nil, err
	}

	var result models.ArticleSchema
	if err := json.Unmarshal([]byte(resp.Data), &result); err != nil {
		return nil, err
	}
	return &result, nil
}

func (e *LLMExtractor) extractArticleHeuristic(html string) (*models.ArticleSchema, error) {
	doc, err := goquery.NewDocumentFromReader(strings.NewReader(html))
	if err != nil {
		return &models.ArticleSchema{}, nil
	}

	result := &models.ArticleSchema{}

	// Title: article h1 > og:title > title
	result.Title = strings.TrimSpace(doc.Find("article h1, .post-title, .entry-title").First().Text())
	if result.Title == "" {
		result.Title = metaContent(doc, "og:title")
	}
	if result.Title == "" {
		result.Title = strings.TrimSpace(doc.Find("title").First().Text())
	}

	// Author
	result.Author = strings.TrimSpace(doc.Find("[rel='author'], .author, .byline, [itemprop='author']").First().Text())

	// Published date
	timeEl := doc.Find("article time, [itemprop='datePublished'], .post-date, .published-date").First()
	if dt, exists := timeEl.Attr("datetime"); exists {
		result.PublishedDate = dt
	} else {
		result.PublishedDate = strings.TrimSpace(timeEl.Text())
	}

	// Summary: og:description or meta description
	result.Summary = metaContent(doc, "og:description")
	if result.Summary == "" {
		result.Summary = metaNameContent(doc, "description")
	}

	// Body text from article or main content area
	body := strings.TrimSpace(doc.Find("article, .post-content, .entry-content, .article-body, main").First().Text())
	if len(body) > 5000 {
		body = body[:5000]
	}
	result.Body = body

	// Tags from meta keywords or tag links
	keywords := metaNameContent(doc, "keywords")
	if keywords != "" {
		for _, kw := range strings.Split(keywords, ",") {
			kw = strings.TrimSpace(kw)
			if kw != "" {
				result.Tags = append(result.Tags, kw)
			}
		}
	}

	return result, nil
}

// ExtractContactInfo extracts contact information from HTML.
func (e *LLMExtractor) ExtractContactInfo(ctx context.Context, html string) (*models.ContactPageSchema, error) {
	if e.aiClient != nil {
		result, err := e.extractContactWithAI(ctx, html)
		if err == nil {
			log.Debug().Str("method", "ai").Msg("contact extraction successful via ai-core")
			return result, nil
		}
		log.Warn().Err(err).Msg("ai contact extraction failed, falling back to heuristic")
	}
	return e.extractContactHeuristic(html)
}

func (e *LLMExtractor) extractContactWithAI(ctx context.Context, html string) (*models.ContactPageSchema, error) {
	schema := `{
		"type": "object",
		"properties": {
			"emails": {"type": "array", "items": {"type": "string"}},
			"phones": {"type": "array", "items": {"type": "string"}},
			"addresses": {"type": "array", "items": {"type": "string"}},
			"office_hours": {"type": "string"},
			"social_links": {"type": "array", "items": {"type": "string"}},
			"company_name": {"type": "string"},
			"map_url": {"type": "string"}
		}
	}`

	extractCtx, cancel := context.WithTimeout(ctx, time.Duration(e.aiTimeoutSec)*time.Second)
	defer cancel()

	resp, err := e.aiClient.ExtractData(extractCtx, &ai.ExtractRequest{
		HTML:   html,
		Schema: schema,
	})
	if err != nil {
		return nil, err
	}

	var result models.ContactPageSchema
	if err := json.Unmarshal([]byte(resp.Data), &result); err != nil {
		return nil, err
	}
	return &result, nil
}

func (e *LLMExtractor) extractContactHeuristic(html string) (*models.ContactPageSchema, error) {
	doc, err := goquery.NewDocumentFromReader(strings.NewReader(html))
	if err != nil {
		return &models.ContactPageSchema{}, nil
	}

	result := &models.ContactPageSchema{}

	// Company name
	result.CompanyName = metaContent(doc, "og:site_name")
	if result.CompanyName == "" {
		result.CompanyName = strings.TrimSpace(doc.Find("title").First().Text())
	}

	// Emails
	doc.Find("a[href^='mailto:']").Each(func(_ int, s *goquery.Selection) {
		href, _ := s.Attr("href")
		email := strings.TrimPrefix(href, "mailto:")
		if email != "" {
			result.Emails = append(result.Emails, email)
		}
	})

	// Phones
	doc.Find("a[href^='tel:']").Each(func(_ int, s *goquery.Selection) {
		href, _ := s.Attr("href")
		phone := strings.TrimPrefix(href, "tel:")
		if phone != "" {
			result.Phones = append(result.Phones, phone)
		}
	})

	// Social links
	socialDomains := []string{"linkedin.com", "twitter.com", "x.com", "facebook.com", "instagram.com", "github.com", "youtube.com"}
	doc.Find("a[href]").Each(func(_ int, s *goquery.Selection) {
		href, _ := s.Attr("href")
		for _, domain := range socialDomains {
			if strings.Contains(href, domain) {
				result.SocialLinks = append(result.SocialLinks, href)
				break
			}
		}
	})

	// Google Maps URL
	doc.Find("a[href*='google.com/maps'], a[href*='maps.google'], iframe[src*='google.com/maps']").Each(func(_ int, s *goquery.Selection) {
		if href, exists := s.Attr("href"); exists && result.MapURL == "" {
			result.MapURL = href
		}
		if src, exists := s.Attr("src"); exists && result.MapURL == "" {
			result.MapURL = src
		}
	})

	// Address from JSON-LD
	doc.Find("script[type='application/ld+json']").Each(func(_ int, s *goquery.Selection) {
		var ld map[string]interface{}
		if err := json.Unmarshal([]byte(s.Text()), &ld); err != nil {
			return
		}
		if addr, ok := ld["address"].(map[string]interface{}); ok {
			var parts []string
			for _, key := range []string{"streetAddress", "addressLocality", "postalCode", "addressCountry"} {
				if v, ok := addr[key].(string); ok && v != "" {
					parts = append(parts, v)
				}
			}
			if len(parts) > 0 {
				result.Addresses = append(result.Addresses, strings.Join(parts, ", "))
			}
		}
	})

	return result, nil
}

// ExtractServices extracts service/offering information from HTML.
func (e *LLMExtractor) ExtractServices(ctx context.Context, html string) (*models.ServicePageSchema, error) {
	if e.aiClient != nil {
		result, err := e.extractServicesWithAI(ctx, html)
		if err == nil {
			log.Debug().Str("method", "ai").Msg("services extraction successful via ai-core")
			return result, nil
		}
		log.Warn().Err(err).Msg("ai services extraction failed, falling back to heuristic")
	}
	return e.extractServicesHeuristic(html)
}

func (e *LLMExtractor) extractServicesWithAI(ctx context.Context, html string) (*models.ServicePageSchema, error) {
	schema := `{
		"type": "object",
		"properties": {
			"company_name": {"type": "string"},
			"services": {"type": "array", "items": {
				"type": "object",
				"properties": {
					"name": {"type": "string"},
					"description": {"type": "string"},
					"pricing_hint": {"type": "string"}
				}
			}}
		}
	}`

	extractCtx, cancel := context.WithTimeout(ctx, time.Duration(e.aiTimeoutSec)*time.Second)
	defer cancel()

	resp, err := e.aiClient.ExtractData(extractCtx, &ai.ExtractRequest{
		HTML:   html,
		Schema: schema,
	})
	if err != nil {
		return nil, err
	}

	var result models.ServicePageSchema
	if err := json.Unmarshal([]byte(resp.Data), &result); err != nil {
		return nil, err
	}
	return &result, nil
}

func (e *LLMExtractor) extractServicesHeuristic(html string) (*models.ServicePageSchema, error) {
	doc, err := goquery.NewDocumentFromReader(strings.NewReader(html))
	if err != nil {
		return &models.ServicePageSchema{}, nil
	}

	result := &models.ServicePageSchema{}

	result.CompanyName = metaContent(doc, "og:site_name")
	if result.CompanyName == "" {
		result.CompanyName = strings.TrimSpace(doc.Find("title").First().Text())
	}

	// Extract services from h2/h3 headings with sibling descriptions
	doc.Find("h2, h3").Each(func(_ int, s *goquery.Selection) {
		name := strings.TrimSpace(s.Text())
		if name == "" || len(name) > 200 {
			return
		}
		// Look for description in the next sibling paragraph
		desc := strings.TrimSpace(s.Next().Text())
		if len(desc) > 500 {
			desc = desc[:500]
		}
		result.Services = append(result.Services, models.ServiceDetail{
			Name:        name,
			Description: desc,
		})
	})

	// Limit to top 20 services
	if len(result.Services) > 20 {
		result.Services = result.Services[:20]
	}

	return result, nil
}

// ExtractGenericContent extracts general page content from HTML.
func (e *LLMExtractor) ExtractGenericContent(ctx context.Context, html string) (*models.GenericPageSchema, error) {
	if e.aiClient != nil {
		result, err := e.extractGenericWithAI(ctx, html)
		if err == nil {
			log.Debug().Str("method", "ai").Msg("generic extraction successful via ai-core")
			return result, nil
		}
		log.Warn().Err(err).Msg("ai generic extraction failed, falling back to heuristic")
	}
	return e.extractGenericHeuristic(html)
}

func (e *LLMExtractor) extractGenericWithAI(ctx context.Context, html string) (*models.GenericPageSchema, error) {
	schema := `{
		"type": "object",
		"properties": {
			"title": {"type": "string"},
			"headings": {"type": "array", "items": {"type": "string"}},
			"main_text": {"type": "string"},
			"key_links": {"type": "array", "items": {"type": "string"}}
		}
	}`

	extractCtx, cancel := context.WithTimeout(ctx, time.Duration(e.aiTimeoutSec)*time.Second)
	defer cancel()

	resp, err := e.aiClient.ExtractData(extractCtx, &ai.ExtractRequest{
		HTML:   html,
		Schema: schema,
	})
	if err != nil {
		return nil, err
	}

	var result models.GenericPageSchema
	if err := json.Unmarshal([]byte(resp.Data), &result); err != nil {
		return nil, err
	}
	return &result, nil
}

func (e *LLMExtractor) extractGenericHeuristic(html string) (*models.GenericPageSchema, error) {
	doc, err := goquery.NewDocumentFromReader(strings.NewReader(html))
	if err != nil {
		return &models.GenericPageSchema{}, nil
	}

	result := &models.GenericPageSchema{}

	// Title
	result.Title = strings.TrimSpace(doc.Find("title").First().Text())
	if result.Title == "" {
		result.Title = strings.TrimSpace(doc.Find("h1").First().Text())
	}

	// Headings (h1-h3)
	doc.Find("h1, h2, h3").Each(func(_ int, s *goquery.Selection) {
		heading := strings.TrimSpace(s.Text())
		if heading != "" && len(heading) < 300 {
			result.Headings = append(result.Headings, heading)
		}
	})
	if len(result.Headings) > 30 {
		result.Headings = result.Headings[:30]
	}

	// Main text from main content area
	mainText := strings.TrimSpace(doc.Find("main, article, .content, #content, .main-content").First().Text())
	if mainText == "" {
		mainText = strings.TrimSpace(doc.Find("body").First().Text())
	}
	if len(mainText) > 5000 {
		mainText = mainText[:5000]
	}
	result.MainText = mainText

	// Key internal links (skip assets, fragments)
	doc.Find("nav a[href], main a[href], .content a[href]").Each(func(_ int, s *goquery.Selection) {
		href, _ := s.Attr("href")
		if href != "" && !strings.HasPrefix(href, "#") && !strings.HasPrefix(href, "javascript:") {
			result.KeyLinks = append(result.KeyLinks, href)
		}
	})
	if len(result.KeyLinks) > 50 {
		result.KeyLinks = result.KeyLinks[:50]
	}

	return result, nil
}

// extractMainContent extracts cleaned main text from a page (universal, for Content field).
func extractMainContent(doc *goquery.Document) string {
	// Remove script, style, nav, footer, header to get cleaner text
	clone := doc.Clone()
	clone.Find("script, style, nav, footer, header, noscript, iframe").Remove()

	text := strings.TrimSpace(clone.Find("main, article, .content, #content, .main-content").First().Text())
	if text == "" {
		text = strings.TrimSpace(clone.Find("body").First().Text())
	}

	// Clean up whitespace
	lines := strings.Split(text, "\n")
	var cleaned []string
	for _, line := range lines {
		line = strings.TrimSpace(line)
		if line != "" {
			cleaned = append(cleaned, line)
		}
	}
	text = strings.Join(cleaned, "\n")

	if len(text) > 5000 {
		text = text[:5000]
	}
	return text
}

func extractPrice(input string) int {
	re := regexp.MustCompile(`\d+[\.,]?\d*`)
	match := re.FindString(input)
	if match == "" {
		return 0
	}
	match = strings.ReplaceAll(match, ".", "")
	match = strings.ReplaceAll(match, ",", ".")
	v, err := strconv.ParseFloat(match, 64)
	if err != nil {
		return 0
	}
	return int(v)
}
