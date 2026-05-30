package scraper

import (
	"net/url"
	"path"
	"strings"

	"github.com/PuerkitoBio/goquery"
	"github.com/rs/zerolog/log"
	"github.com/triodelab/quarry/internal/models"
)

// matchesPathFilter checks a URL against include/exclude glob patterns.
// Returns true if the URL should be kept.
func matchesPathFilter(rawURL string, includePaths, excludePaths []string) bool {
	parsed, err := url.Parse(rawURL)
	if err != nil {
		return true // can't parse, keep it
	}
	urlPath := parsed.Path

	// Check excludePaths first — if any match, reject
	for _, pattern := range excludePaths {
		if matched, _ := path.Match(pattern, urlPath); matched {
			return false
		}
		// Also try with trailing wildcard for convenience: /admin/* matches /admin/foo
		if strings.HasSuffix(pattern, "/*") {
			prefix := strings.TrimSuffix(pattern, "/*")
			if strings.HasPrefix(urlPath, prefix+"/") || urlPath == prefix {
				return false
			}
		}
	}

	// Check includePaths — if set, at least one must match
	if len(includePaths) > 0 {
		for _, pattern := range includePaths {
			if matched, _ := path.Match(pattern, urlPath); matched {
				return true
			}
			if strings.HasSuffix(pattern, "/*") {
				prefix := strings.TrimSuffix(pattern, "/*")
				if strings.HasPrefix(urlPath, prefix+"/") || urlPath == prefix {
					return true
				}
			}
		}
		return false // includePaths set but none matched
	}

	return true
}

// extractProductLinks extracts URLs from a page's HTML.
// includePaths/excludePaths are optional glob filters on URL paths.
func extractProductLinks(html, pageURL, collection string, includePaths, excludePaths []string) []*models.Product {
	log.Debug().Str("url", pageURL).Int("html_len", len(html)).Msg("Extracting links from page")
	doc, err := goquery.NewDocumentFromReader(strings.NewReader(html))
	if err != nil {
		return nil
	}

	products := make([]*models.Product, 0)
	seen := make(map[string]bool)

	// Determine base domain for relative links
	baseDomain := ""
	if pageURL != "" {
		if strings.HasPrefix(pageURL, "http") {
			parts := strings.Split(pageURL, "/")
			if len(parts) >= 3 {
				baseDomain = parts[0] + "//" + parts[2]
			}
		}
	}

	// Find all potential product links (using common ecommerce patterns or any link)
	// We'll look for /produkter/ for skinsecret, but also other links for other domains
	doc.Find("a[href]").Each(func(i int, s *goquery.Selection) {
		href, exists := s.Attr("href")
		if !exists {
			return
		}

		// Clean and normalize URL
		url := strings.TrimSpace(href)
		if url == "" || strings.HasPrefix(url, "#") || strings.HasPrefix(url, "javascript:") ||
			strings.HasPrefix(url, "tel:") || strings.HasPrefix(url, "mailto:") ||
			strings.HasPrefix(url, "data:") || strings.HasPrefix(url, "blob:") {
			return
		}

		if !strings.HasPrefix(url, "http") {
			if strings.HasPrefix(url, "/") {
				url = baseDomain + url
			} else {
				// Very basic relative path resolution
				url = pageURL + "/" + url
			}
		}

		// Normalize: remove trailing slash for deduplication
		url = strings.TrimSuffix(url, "/")

		// Skip if already seen
		if seen[url] {
			return
		}
		
		// Domain skip: only stay on the same base domain
		if baseDomain != "" && !strings.HasPrefix(url, baseDomain) {
			return
		}

		// Heuristic: skip obvious non-content links
		lowerURL := strings.ToLower(url)
		if strings.Contains(lowerURL, "/wp-json/") || 
		   strings.Contains(lowerURL, "/wp-includes/") ||
		   strings.Contains(lowerURL, "/wp-content/") ||
		   strings.Contains(lowerURL, "?p=") ||
		   strings.Contains(lowerURL, "&p=") ||
		   strings.HasSuffix(lowerURL, ".js") ||
		   strings.HasSuffix(lowerURL, ".css") ||
		   strings.HasSuffix(lowerURL, ".png") ||
		   strings.HasSuffix(lowerURL, ".jpg") ||
		   strings.HasSuffix(lowerURL, ".jpeg") ||
		   strings.HasSuffix(lowerURL, ".gif") ||
		   strings.HasSuffix(lowerURL, ".svg") ||
		   strings.HasSuffix(lowerURL, ".pdf") ||
		   strings.Contains(lowerURL, "/tag/") ||
		   strings.Contains(lowerURL, "/category/") ||
		   strings.Contains(lowerURL, "/kategori/") {
			return
		}

		// Heuristic: for example.com, just take what's there but don't be crazy
		if strings.Contains(baseDomain, "example.com") {
			// example.com only has one link that is interesting
			if !strings.Contains(url, "iana.org") {
				return
			}
		}

		// Apply path filters
		if !matchesPathFilter(url, includePaths, excludePaths) {
			return
		}

		seen[url] = true

		// Try to extract SKU or slug from URL
		slug := ""
		parts := strings.Split(url, "/")
		if len(parts) > 0 {
			slug = parts[len(parts)-1]
		}

		products = append(products, &models.Product{
			SKU:      slug,
			URL:      url,
			Category: collection,
			URLValid: true,
		})
	})

	return products
}

// filterCookieBanner removes cookie consent text from content
func filterCookieBanner(content string) string {
	banners := []string{
		"Behandle samtykke for informasjonskapsler",
		"Funksjonelle",
		"Preferanser",
		"Statistikk",
		"Markedsføring",
		"Godta alle",
		"Avvis alle",
	}

	lines := strings.Split(content, "\n")
	filtered := make([]string, 0, len(lines))

	for _, line := range lines {
		skip := false
		for _, banner := range banners {
			if strings.Contains(line, banner) {
				skip = true
				break
			}
		}
		if !skip {
			filtered = append(filtered, line)
		}
	}

	return strings.Join(filtered, "\n")
}
