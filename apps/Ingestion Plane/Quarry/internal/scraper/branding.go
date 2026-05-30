package scraper

import (
	"net/url"
	"regexp"
	"strings"

	"github.com/PuerkitoBio/goquery"
)

// BrandIdentity is the enriched brand extraction result.
type BrandIdentity struct {
	URL          string            `json:"url"`
	Title        string            `json:"title,omitempty"`
	SiteName     string            `json:"siteName,omitempty"`
	BrandName    string            `json:"brandName,omitempty"`
	Description  string            `json:"description,omitempty"`
	Logo         string            `json:"logo,omitempty"`
	Favicon      string            `json:"favicon,omitempty"`
	ThemeColor   string            `json:"themeColor,omitempty"`
	Colors       []string          `json:"colors,omitempty"`
	Fonts        []string          `json:"fonts,omitempty"`
	Language     string            `json:"language,omitempty"`
	CanonicalURL string            `json:"canonicalUrl,omitempty"`
	SocialLinks  map[string]string `json:"socialLinks,omitempty"`
	SchemaOrg    map[string]string `json:"schemaOrg,omitempty"`
}

var (
	hexColorRe  = regexp.MustCompile(`#([0-9a-fA-F]{3,8})\b`)
	fontFamilyRe = regexp.MustCompile(`font-family\s*:\s*([^;}{]+)`)
)

func extractBranding(targetURL, html string) map[string]interface{} {
	brand := BrandIdentity{URL: targetURL}
	if strings.TrimSpace(html) == "" {
		return brandToMap(brand)
	}

	doc, err := goquery.NewDocumentFromReader(strings.NewReader(html))
	if err != nil {
		return brandToMap(brand)
	}

	// --- Basic metadata ---
	brand.Title = strings.TrimSpace(doc.Find("title").First().Text())

	brand.SiteName = firstNonEmpty(
		doc.Find(`meta[property="og:site_name"]`).AttrOr("content", ""),
		doc.Find(`meta[name="application-name"]`).AttrOr("content", ""),
		deriveBrandName(targetURL, brand.Title),
	)
	brand.BrandName = brand.SiteName

	brand.Description = firstNonEmpty(
		doc.Find(`meta[name="description"]`).AttrOr("content", ""),
		doc.Find(`meta[property="og:description"]`).AttrOr("content", ""),
	)

	// --- Logo / icons ---
	brand.Logo = resolveAssetURL(targetURL, firstNonEmpty(
		doc.Find(`meta[property="og:image"]`).AttrOr("content", ""),
		doc.Find(`link[rel="icon"][sizes="192x192"]`).AttrOr("href", ""),
		doc.Find(`link[rel="apple-touch-icon"]`).AttrOr("href", ""),
	))
	brand.Favicon = resolveAssetURL(targetURL, firstNonEmpty(
		doc.Find(`link[rel="icon"]`).AttrOr("href", ""),
		doc.Find(`link[rel="shortcut icon"]`).AttrOr("href", ""),
	))

	// --- Theme color ---
	brand.ThemeColor = firstNonEmpty(
		doc.Find(`meta[name="theme-color"]`).AttrOr("content", ""),
		doc.Find(`meta[name="msapplication-TileColor"]`).AttrOr("content", ""),
	)

	// --- CSS colors extraction ---
	colorSet := make(map[string]struct{})
	if brand.ThemeColor != "" {
		colorSet[strings.ToLower(brand.ThemeColor)] = struct{}{}
	}
	doc.Find("style").Each(func(_ int, s *goquery.Selection) {
		css := s.Text()
		for _, match := range hexColorRe.FindAllString(css, 20) {
			lower := strings.ToLower(match)
			// Skip near-black/white/grey as they're not brand colors.
			if lower != "#000" && lower != "#fff" && lower != "#ffffff" && lower != "#000000" {
				colorSet[lower] = struct{}{}
			}
		}
	})
	// Also check inline style attrs on key elements.
	doc.Find(`[style*="color"]`).Each(func(_ int, s *goquery.Selection) {
		style := s.AttrOr("style", "")
		for _, match := range hexColorRe.FindAllString(style, 5) {
			colorSet[strings.ToLower(match)] = struct{}{}
		}
	})
	for c := range colorSet {
		brand.Colors = append(brand.Colors, c)
	}

	// --- Font families ---
	fontSet := make(map[string]struct{})
	doc.Find("style").Each(func(_ int, s *goquery.Selection) {
		css := s.Text()
		for _, match := range fontFamilyRe.FindAllStringSubmatch(css, 10) {
			if len(match) > 1 {
				family := cleanFontFamily(match[1])
				if family != "" {
					fontSet[family] = struct{}{}
				}
			}
		}
	})
	// Inline styles.
	doc.Find(`[style*="font-family"]`).Each(func(_ int, s *goquery.Selection) {
		style := s.AttrOr("style", "")
		for _, match := range fontFamilyRe.FindAllStringSubmatch(style, 5) {
			if len(match) > 1 {
				family := cleanFontFamily(match[1])
				if family != "" {
					fontSet[family] = struct{}{}
				}
			}
		}
	})
	for f := range fontSet {
		brand.Fonts = append(brand.Fonts, f)
	}

	// --- Language ---
	brand.Language = strings.TrimSpace(doc.Find("html").First().AttrOr("lang", ""))

	// --- Canonical URL ---
	brand.CanonicalURL = resolveAssetURL(targetURL, doc.Find(`link[rel="canonical"]`).AttrOr("href", ""))

	// --- Social links ---
	brand.SocialLinks = extractSocialLinks(doc, targetURL)

	// --- Schema.org Organization ---
	brand.SchemaOrg = extractSchemaOrg(doc)

	return brandToMap(brand)
}

func extractSocialLinks(doc *goquery.Document, baseURL string) map[string]string {
	socials := map[string]string{}
	socialDomains := map[string]string{
		"twitter.com": "twitter", "x.com": "twitter",
		"facebook.com": "facebook", "instagram.com": "instagram",
		"linkedin.com": "linkedin", "youtube.com": "youtube",
		"github.com": "github", "tiktok.com": "tiktok",
		"threads.net": "threads", "bsky.app": "bluesky",
	}

	doc.Find("a[href]").Each(func(_ int, s *goquery.Selection) {
		href := strings.TrimSpace(s.AttrOr("href", ""))
		if href == "" {
			return
		}
		parsed, err := url.Parse(href)
		if err != nil || !parsed.IsAbs() {
			return
		}
		host := strings.TrimPrefix(strings.ToLower(parsed.Hostname()), "www.")
		if platform, ok := socialDomains[host]; ok {
			if _, exists := socials[platform]; !exists {
				socials[platform] = href
			}
		}
	})
	return socials
}

func extractSchemaOrg(doc *goquery.Document) map[string]string {
	schema := map[string]string{}
	doc.Find(`[itemtype*="schema.org/Organization"]`).First().Each(func(_ int, s *goquery.Selection) {
		if name := strings.TrimSpace(s.Find(`[itemprop="name"]`).Text()); name != "" {
			schema["name"] = name
		}
		if logo := strings.TrimSpace(s.Find(`[itemprop="logo"]`).AttrOr("content", "")); logo != "" {
			schema["logo"] = logo
		} else if logo := strings.TrimSpace(s.Find(`[itemprop="logo"] img`).AttrOr("src", "")); logo != "" {
			schema["logo"] = logo
		}
	})
	return schema
}

func cleanFontFamily(raw string) string {
	// Take first font in the fallback chain, strip quotes.
	parts := strings.SplitN(raw, ",", 2)
	f := strings.TrimSpace(parts[0])
	f = strings.Trim(f, `"'`)
	f = strings.TrimSpace(f)
	// Skip generic families.
	lower := strings.ToLower(f)
	if lower == "serif" || lower == "sans-serif" || lower == "monospace" || lower == "cursive" || lower == "system-ui" || lower == "inherit" {
		return ""
	}
	return f
}

func firstNonEmpty(values ...string) string {
	for _, v := range values {
		trimmed := strings.TrimSpace(v)
		if trimmed != "" {
			return trimmed
		}
	}
	return ""
}

func brandToMap(b BrandIdentity) map[string]interface{} {
	m := map[string]interface{}{
		"url": b.URL,
	}
	if b.Title != "" {
		m["title"] = b.Title
	}
	if b.SiteName != "" {
		m["siteName"] = b.SiteName
	}
	if b.BrandName != "" {
		m["brandName"] = b.BrandName
	}
	if b.Description != "" {
		m["description"] = b.Description
	}
	if b.Logo != "" {
		m["logo"] = b.Logo
	}
	if b.Favicon != "" {
		m["favicon"] = b.Favicon
	}
	if b.ThemeColor != "" {
		m["themeColor"] = b.ThemeColor
	}
	if len(b.Colors) > 0 {
		m["colors"] = b.Colors
	}
	if len(b.Fonts) > 0 {
		m["fonts"] = b.Fonts
	}
	if b.Language != "" {
		m["language"] = b.Language
	}
	if b.CanonicalURL != "" {
		m["canonicalUrl"] = b.CanonicalURL
	}
	if len(b.SocialLinks) > 0 {
		m["socialLinks"] = b.SocialLinks
	}
	if len(b.SchemaOrg) > 0 {
		m["schemaOrg"] = b.SchemaOrg
	}
	return m
}

func deriveBrandName(targetURL, title string) string {
	if trimmed := strings.TrimSpace(title); trimmed != "" {
		for _, sep := range []string{" | ", " - ", " — ", " · "} {
			if head, _, found := strings.Cut(trimmed, sep); found && strings.TrimSpace(head) != "" {
				return strings.TrimSpace(head)
			}
		}
	}

	parsed, err := url.Parse(targetURL)
	if err != nil {
		return ""
	}
	host := strings.TrimPrefix(parsed.Hostname(), "www.")
	if host == "" {
		return ""
	}
	firstLabel, _, _ := strings.Cut(host, ".")
	if firstLabel == "" {
		return ""
	}
	return strings.Title(strings.ReplaceAll(firstLabel, "-", " "))
}

func resolveAssetURL(baseURL, asset string) string {
	asset = strings.TrimSpace(asset)
	if asset == "" {
		return ""
	}
	parsedAsset, err := url.Parse(asset)
	if err != nil {
		return asset
	}
	if parsedAsset.IsAbs() {
		return parsedAsset.String()
	}
	parsedBase, err := url.Parse(baseURL)
	if err != nil {
		return asset
	}
	return parsedBase.ResolveReference(parsedAsset).String()
}
