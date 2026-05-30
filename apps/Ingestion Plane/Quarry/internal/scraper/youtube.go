package scraper

import (
	"net/url"
	"regexp"
	"strings"

	"github.com/PuerkitoBio/goquery"
)

var (
	ytIDFromURL  = regexp.MustCompile(`(?:v=|\/embed\/|\/v\/|youtu\.be\/|\/shorts\/)([A-Za-z0-9_-]{11})`)
	ytIDInline   = regexp.MustCompile(`^[A-Za-z0-9_-]{11}$`)
)

// extractYouTubeMetadata extracts YouTube-specific metadata from a page.
// Works for youtube.com/watch?v=… pages as well as pages that embed a YouTube
// player via an <iframe> or <link rel="canonical"> pointing to YouTube.
func extractYouTubeMetadata(targetURL, html string) map[string]interface{} {
	result := map[string]interface{}{
		"url":              targetURL,
		"videoId":          "",
		"title":            "",
		"description":      "",
		"publishedAt":      "",
		"author":           "",
		"channelURL":       "",
		"thumbnailURL":     "",
		"duration":         "",
		"viewCount":        "",
		"likeCount":        "",
		"tags":             []string{},
		"category":         "",
		"isLive":           false,
		"isMembersOnly":    false,
		"embedURL":         "",
	}

	if strings.TrimSpace(html) == "" {
		return result
	}

	doc, err := goquery.NewDocumentFromReader(strings.NewReader(html))
	if err != nil {
		return result
	}

	// ── Video ID Resolution ────────────────────────────────────────────────

	videoID := extractYouTubeID(targetURL)
	if videoID == "" {
		// Try canonical URL
		if canonical := doc.Find(`link[rel="canonical"]`).AttrOr("href", ""); canonical != "" {
			videoID = extractYouTubeID(canonical)
		}
	}
	if videoID == "" {
		// Try embedded <iframe> src
		doc.Find(`iframe`).Each(func(_ int, sel *goquery.Selection) {
			if src := sel.AttrOr("src", ""); strings.Contains(src, "youtube") || strings.Contains(src, "youtu.be") {
				if id := extractYouTubeID(src); id != "" {
					videoID = id
				}
			}
		})
	}
	result["videoId"] = videoID

	if videoID != "" {
		result["embedURL"] = "https://www.youtube.com/embed/" + videoID
	}

	// ── Standard meta tags ────────────────────────────────────────────────

	setIfNonEmpty := func(key, value string) {
		if strings.TrimSpace(value) != "" {
			result[key] = strings.TrimSpace(value)
		}
	}

	setIfNonEmpty("title", doc.Find("title").First().Text())

	for _, nameAttr := range []struct{ name, key string }{
		{"description", "description"},
		{"keywords", "tags"},
	} {
		v := doc.Find(`meta[name="` + nameAttr.name + `"]`).AttrOr("content", "")
		setIfNonEmpty(nameAttr.key, v)
	}

	// Open Graph
	for _, prop := range []struct{ property, key string }{
		{"og:title", "title"},
		{"og:description", "description"},
		{"og:image", "thumbnailURL"},
		{"og:video:duration", "duration"},
	} {
		v := doc.Find(`meta[property="` + prop.property + `"]`).AttrOr("content", "")
		setIfNonEmpty(prop.key, v)
	}

	// Twitter / YouTube-specific itemprop
	for _, itemprop := range []struct{ prop, key string }{
		{"name", "title"},
		{"description", "description"},
		{"uploadDate", "publishedAt"},
		{"duration", "duration"},
		{"thumbnailUrl", "thumbnailURL"},
		{"author", "author"},
		{"interactionCount", "viewCount"},
	} {
		sel := doc.Find(`[itemprop="` + itemprop.prop + `"]`)
		v := strings.TrimSpace(sel.First().AttrOr("content", sel.First().Text()))
		setIfNonEmpty(itemprop.key, v)
	}

	// Channel info from <link rel="author">
	if authorHref := doc.Find(`link[rel="author"]`).AttrOr("href", ""); authorHref != "" {
		setIfNonEmpty("channelURL", resolveAssetURL(targetURL, authorHref))
	}

	// Published date from <meta itemprop="datePublished">
	if pub := doc.Find(`meta[itemprop="datePublished"]`).AttrOr("content", ""); pub != "" {
		setIfNonEmpty("publishedAt", pub)
	}
	// Also try <time> element
	if pub := doc.Find(`time[itemprop="datePublished"], time[datetime]`).First().AttrOr("datetime", ""); pub != "" {
		setIfNonEmpty("publishedAt", pub)
	}

	// Tags — split comma-separated keywords
	if tagsStr, ok := result["tags"].(string); ok && tagsStr != "" {
		parts := strings.Split(tagsStr, ",")
		tags := make([]string, 0, len(parts))
		for _, p := range parts {
			if t := strings.TrimSpace(p); t != "" {
				tags = append(tags, t)
			}
		}
		result["tags"] = tags
	}

	return result
}

// extractYouTubeID extracts an 11-character YouTube video ID from a URL.
func extractYouTubeID(rawURL string) string {
	if ytIDInline.MatchString(rawURL) {
		return rawURL
	}
	if m := ytIDFromURL.FindStringSubmatch(rawURL); len(m) >= 2 {
		return m[1]
	}
	// Fallback: parse URL and check v= query param
	if u, err := url.Parse(rawURL); err == nil {
		if v := u.Query().Get("v"); ytIDInline.MatchString(v) {
			return v
		}
	}
	return ""
}
