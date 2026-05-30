package extractor

import (
	"encoding/json"
	"net/url"
	"strings"

	"github.com/PuerkitoBio/goquery"

	"github.com/triodelab/quarry/internal/models"
)

// ClassifyPage determines the page type from URL patterns and HTML signals.
// Returns (pageType, confidence) where confidence is 0.0–1.0.
//
// Strategy: URL gives a specific page purpose (contact, services, team, etc.).
// HTML signals (JSON-LD, og:type, microdata) confirm or override.
// However, generic site-wide signals like Organization JSON-LD should NOT
// override a specific URL classification — every page on a company site has
// Organization JSON-LD, but /kontakt is still a contact page.
func ClassifyPage(pageURL string, doc *goquery.Document) (string, float64) {
	urlType, urlConf := classifyByURL(pageURL)
	htmlType, htmlConf := classifyByHTML(doc)

	// If URL gives a specific page type (not generic/homepage), only let HTML
	// override if it gives an equally specific or more specific type.
	// Organization/LocalBusiness JSON-LD → "about" is generic (site-wide),
	// so it should not override contact/services/team/pricing URL signals.
	urlIsSpecific := urlConf >= 0.6 && urlType != models.PageTypeGeneric && urlType != models.PageTypeHomepage
	htmlIsGenericCompany := htmlType == models.PageTypeAbout && htmlConf <= 0.85

	if urlIsSpecific && htmlIsGenericCompany {
		// URL says "contact" or "services" etc., HTML only says "about" from
		// site-wide Organization JSON-LD. Trust the URL.
		return urlType, urlConf
	}

	// Otherwise, use the higher-confidence signal
	if htmlConf > urlConf {
		return htmlType, htmlConf
	}
	return urlType, urlConf
}

// classifyByURL uses URL path patterns to guess page type.
func classifyByURL(pageURL string) (string, float64) {
	parsed, err := url.Parse(strings.ToLower(pageURL))
	if err != nil {
		return models.PageTypeGeneric, 0.1
	}

	path := strings.Trim(parsed.Path, "/")

	// Root path → homepage
	if path == "" {
		return models.PageTypeHomepage, 0.7
	}

	segments := strings.Split(path, "/")
	first := segments[0]

	// About pages (EN + NO)
	aboutPatterns := []string{"about", "about-us", "om-oss", "om", "over-ons", "uber-uns", "qui-sommes-nous"}
	for _, p := range aboutPatterns {
		if first == p {
			return models.PageTypeAbout, 0.7
		}
	}

	// Contact pages (EN + NO)
	contactPatterns := []string{"contact", "contact-us", "kontakt", "kontakta-oss", "kontakt-oss"}
	for _, p := range contactPatterns {
		if first == p {
			return models.PageTypeContact, 0.7
		}
	}

	// Article / blog pages
	articlePatterns := []string{"blog", "artikkel", "article", "articles", "news", "nyheter", "posts", "aktuelt", "magazine", "journal"}
	for _, p := range articlePatterns {
		if first == p || (len(segments) > 1 && first == p) {
			return models.PageTypeArticle, 0.6
		}
	}

	// Product pages
	productPatterns := []string{"product", "products", "produkt", "produkter", "shop", "butikk", "store", "produktkategori", "collections"}
	for _, p := range productPatterns {
		if first == p {
			return models.PageTypeProduct, 0.6
		}
	}

	// Services pages (EN + NO)
	servicePatterns := []string{"services", "service", "tjenester", "losninger", "solutions", "what-we-do", "hva-vi-gjor"}
	for _, p := range servicePatterns {
		if first == p {
			return models.PageTypeServices, 0.7
		}
	}

	// Team pages
	teamPatterns := []string{"team", "ansatte", "people", "about-us/team", "medarbeidere", "our-team", "staff"}
	for _, p := range teamPatterns {
		if first == p || path == p {
			return models.PageTypeTeam, 0.7
		}
	}

	// Pricing pages
	pricingPatterns := []string{"pricing", "priser", "plans", "packages", "prisliste"}
	for _, p := range pricingPatterns {
		if first == p {
			return models.PageTypePricing, 0.7
		}
	}

	return models.PageTypeGeneric, 0.3
}

// classifyByHTML uses structured data, meta tags, and DOM signals.
func classifyByHTML(doc *goquery.Document) (string, float64) {
	if doc == nil {
		return models.PageTypeGeneric, 0.0
	}

	// 1. JSON-LD @type — strongest signal
	pageType, conf := classifyByJSONLD(doc)
	if conf > 0 {
		return pageType, conf
	}

	// 2. og:type meta tag
	ogType := metaContent(doc, "og:type")
	switch strings.ToLower(ogType) {
	case "article":
		return models.PageTypeArticle, 0.8
	case "product":
		return models.PageTypeProduct, 0.8
	case "profile":
		return models.PageTypeTeam, 0.6
	}

	// 3. Product microdata signals
	hasPrice := doc.Find("[itemprop='price'], [itemprop='priceCurrency'], .product-price, .price").Length() > 0
	hasAddToCart := doc.Find("[class*='add-to-cart'], [id*='add-to-cart'], button[name='add']").Length() > 0
	if hasPrice && hasAddToCart {
		return models.PageTypeProduct, 0.8
	}
	if hasPrice {
		return models.PageTypeProduct, 0.6
	}

	// 4. Article signals
	hasArticleTag := doc.Find("article").Length() > 0
	hasTimeTag := doc.Find("article time, article [datetime], .post-date, .published-date").Length() > 0
	if hasArticleTag && hasTimeTag {
		return models.PageTypeArticle, 0.7
	}

	// 5. Contact form signals
	hasContactForm := false
	doc.Find("form").Each(func(_ int, s *goquery.Selection) {
		formHTML, _ := s.Html()
		lower := strings.ToLower(formHTML)
		if strings.Contains(lower, "email") || strings.Contains(lower, "e-post") ||
			strings.Contains(lower, "phone") || strings.Contains(lower, "telefon") ||
			strings.Contains(lower, "message") || strings.Contains(lower, "melding") {
			hasContactForm = true
		}
	})
	if hasContactForm {
		return models.PageTypeContact, 0.6
	}

	return models.PageTypeGeneric, 0.0
}

// classifyByJSONLD inspects JSON-LD structured data for @type.
func classifyByJSONLD(doc *goquery.Document) (string, float64) {
	bestType := models.PageTypeGeneric
	bestConf := 0.0

	doc.Find("script[type='application/ld+json']").Each(func(_ int, s *goquery.Selection) {
		text := strings.TrimSpace(s.Text())
		if text == "" {
			return
		}

		// Try single object
		var ld map[string]interface{}
		if err := json.Unmarshal([]byte(text), &ld); err == nil {
			t, c := classifyLDType(ld)
			if c > bestConf {
				bestType, bestConf = t, c
			}
			return
		}

		// Try array of objects
		var ldArr []map[string]interface{}
		if err := json.Unmarshal([]byte(text), &ldArr); err == nil {
			for _, item := range ldArr {
				t, c := classifyLDType(item)
				if c > bestConf {
					bestType, bestConf = t, c
				}
			}
		}
	})

	return bestType, bestConf
}

func classifyLDType(ld map[string]interface{}) (string, float64) {
	ldType, _ := ld["@type"].(string)
	switch ldType {
	case "Organization", "LocalBusiness", "Corporation", "GovernmentOrganization":
		return models.PageTypeAbout, 0.85
	case "Product":
		return models.PageTypeProduct, 0.9
	case "Article", "BlogPosting", "NewsArticle", "TechArticle", "WebPage":
		if ldType == "WebPage" {
			return models.PageTypeGeneric, 0.3
		}
		return models.PageTypeArticle, 0.9
	case "ContactPage":
		return models.PageTypeContact, 0.9
	case "AboutPage":
		return models.PageTypeAbout, 0.9
	case "FAQPage":
		return models.PageTypeGeneric, 0.5
	case "Service", "Offer":
		return models.PageTypeServices, 0.85
	case "Person":
		return models.PageTypeTeam, 0.7
	}

	// Handle @graph arrays
	if graph, ok := ld["@graph"].([]interface{}); ok {
		bestType := models.PageTypeGeneric
		bestConf := 0.0
		for _, item := range graph {
			if m, ok := item.(map[string]interface{}); ok {
				t, c := classifyLDType(m)
				if c > bestConf {
					bestType, bestConf = t, c
				}
			}
		}
		return bestType, bestConf
	}

	return models.PageTypeGeneric, 0.0
}

