package scraper

import (
	"strings"

	"github.com/PuerkitoBio/goquery"
)

func analyzeSEO(targetURL, html string) map[string]interface{} {
	result := map[string]interface{}{
		"url":    targetURL,
		"issues": []string{},
	}
	if strings.TrimSpace(html) == "" {
		return result
	}

	doc, err := goquery.NewDocumentFromReader(strings.NewReader(html))
	if err != nil {
		return result
	}

	issues := make([]string, 0)
	title := strings.TrimSpace(doc.Find("title").First().Text())
	result["title"] = title
	result["titleLength"] = len([]rune(title))
	if title == "" {
		issues = append(issues, "missing_title")
	}

	description := strings.TrimSpace(doc.Find(`meta[name="description"]`).AttrOr("content", ""))
	result["metaDescription"] = description
	result["metaDescriptionLength"] = len([]rune(description))
	if description == "" {
		issues = append(issues, "missing_meta_description")
	}

	h1Count := doc.Find("h1").Length()
	result["h1Count"] = h1Count
	if h1Count == 0 {
		issues = append(issues, "missing_h1")
	}
	if h1Count > 1 {
		issues = append(issues, "multiple_h1")
	}

	canonical := strings.TrimSpace(doc.Find(`link[rel="canonical"]`).AttrOr("href", ""))
	if canonical != "" {
		result["canonicalUrl"] = resolveAssetURL(targetURL, canonical)
	} else {
		issues = append(issues, "missing_canonical")
	}

	lang := strings.TrimSpace(doc.Find("html").First().AttrOr("lang", ""))
	result["language"] = lang
	if lang == "" {
		issues = append(issues, "missing_html_lang")
	}

	robots := strings.TrimSpace(doc.Find(`meta[name="robots"]`).AttrOr("content", ""))
	if robots != "" {
		result["robots"] = robots
	}

	openGraph := map[string]string{}
	for _, key := range []string{"og:title", "og:description", "og:image", "og:type", "og:site_name", "og:locale"} {
		value := strings.TrimSpace(doc.Find(`meta[property="`+key+`"]`).AttrOr("content", ""))
		if value != "" {
			openGraph[key] = value
		}
	}
	// og:locale:alternate may appear multiple times
	doc.Find(`meta[property="og:locale:alternate"]`).Each(func(_ int, sel *goquery.Selection) {
		if v := strings.TrimSpace(sel.AttrOr("content", "")); v != "" {
			if existing, ok := openGraph["og:locale:alternate"]; ok {
				openGraph["og:locale:alternate"] = existing + "," + v
			} else {
				openGraph["og:locale:alternate"] = v
			}
		}
	})
	if len(openGraph) > 0 {
		result["openGraph"] = openGraph
	}

	twitter := strings.TrimSpace(doc.Find(`meta[name="twitter:card"]`).AttrOr("content", ""))
	if twitter != "" {
		result["twitterCard"] = twitter
	}

	// Article timestamps
	publishedTime := strings.TrimSpace(doc.Find(`meta[property="article:published_time"]`).AttrOr("content", ""))
	if publishedTime != "" {
		result["publishedTime"] = publishedTime
	}
	modifiedTime := strings.TrimSpace(doc.Find(`meta[property="article:modified_time"]`).AttrOr("content", ""))
	if modifiedTime != "" {
		result["modifiedTime"] = modifiedTime
	}

	// Keywords
	keywords := strings.TrimSpace(doc.Find(`meta[name="keywords"]`).AttrOr("content", ""))
	if keywords != "" {
		result["keywords"] = keywords
	}

	// Dublin Core
	dc := map[string]string{}
	dcMeta := map[string]string{
		"dc.title": "title", "dcterms.title": "title",
		"dc.creator": "creator", "dcterms.creator": "creator", "dc.author": "creator",
		"dc.date": "date", "dcterms.date": "date",
		"dc.description": "description", "dcterms.description": "description",
		"dc.language": "language", "dcterms.language": "language",
		"dc.publisher": "publisher", "dcterms.publisher": "publisher",
		"dc.subject": "subject", "dcterms.subject": "subject",
	}
	doc.Find("meta[name]").Each(func(_ int, sel *goquery.Selection) {
		name := strings.ToLower(strings.TrimSpace(sel.AttrOr("name", "")))
		content := strings.TrimSpace(sel.AttrOr("content", ""))
		if content == "" {
			return
		}
		if dcKey, ok := dcMeta[name]; ok {
			dc[dcKey] = content
		}
	})
	if len(dc) > 0 {
		result["dublinCore"] = dc
	}

	result["structuredDataCount"] = doc.Find(`script[type="application/ld+json"]`).Length()
	result["issues"] = issues
	return result
}

func analyzeWCAG(html string) map[string]interface{} {
	result := map[string]interface{}{
		"issues": []string{},
	}
	if strings.TrimSpace(html) == "" {
		return result
	}

	doc, err := goquery.NewDocumentFromReader(strings.NewReader(html))
	if err != nil {
		return result
	}

	imagesMissingAlt := 0
	doc.Find("img").Each(func(_ int, sel *goquery.Selection) {
		if strings.TrimSpace(sel.AttrOr("alt", "")) == "" {
			imagesMissingAlt++
		}
	})

	inputsMissingLabel := 0
	doc.Find("input, select, textarea").Each(func(_ int, sel *goquery.Selection) {
		id := strings.TrimSpace(sel.AttrOr("id", ""))
		if id == "" {
			if strings.TrimSpace(sel.AttrOr("aria-label", "")) == "" && strings.TrimSpace(sel.AttrOr("aria-labelledby", "")) == "" {
				inputsMissingLabel++
			}
			return
		}
		if doc.Find(`label[for="`+id+`"]`).Length() == 0 &&
			strings.TrimSpace(sel.AttrOr("aria-label", "")) == "" &&
			strings.TrimSpace(sel.AttrOr("aria-labelledby", "")) == "" {
			inputsMissingLabel++
		}
	})

	buttonsMissingName := 0
	doc.Find("button, [role='button']").Each(func(_ int, sel *goquery.Selection) {
		name := strings.TrimSpace(sel.Text())
		if name == "" {
			name = strings.TrimSpace(sel.AttrOr("aria-label", ""))
		}
		if name == "" {
			buttonsMissingName++
		}
	})

	headingOrderIssues := findHeadingOrderIssues(doc)
	titlePresent := strings.TrimSpace(doc.Find("title").First().Text()) != ""
	htmlLangPresent := strings.TrimSpace(doc.Find("html").First().AttrOr("lang", "")) != ""

	issues := make([]string, 0)
	if !titlePresent {
		issues = append(issues, "missing_page_title")
	}
	if !htmlLangPresent {
		issues = append(issues, "missing_html_lang")
	}
	if imagesMissingAlt > 0 {
		issues = append(issues, "images_missing_alt")
	}
	if inputsMissingLabel > 0 {
		issues = append(issues, "form_inputs_missing_label")
	}
	if buttonsMissingName > 0 {
		issues = append(issues, "buttons_missing_accessible_name")
	}
	if headingOrderIssues > 0 {
		issues = append(issues, "heading_order_issues")
	}

	result["titlePresent"] = titlePresent
	result["htmlLangPresent"] = htmlLangPresent
	result["imagesMissingAlt"] = imagesMissingAlt
	result["formInputsMissingLabel"] = inputsMissingLabel
	result["buttonsMissingAccessibleName"] = buttonsMissingName
	result["headingOrderIssues"] = headingOrderIssues
	result["issues"] = issues
	return result
}

func buildPageStatus(targetURL string, statusCode int, contentType string) map[string]interface{} {
	status := "completed"
	if statusCode >= 400 {
		status = "failed_fetch"
	}
	return map[string]interface{}{
		"url":             targetURL,
		"httpStatus":      statusCode,
		"contentType":     contentType,
		"status":          status,
		"duplicateReason": "",
		"blockedReason":   "",
	}
}

func findHeadingOrderIssues(doc *goquery.Document) int {
	lastLevel := 0
	issues := 0
	doc.Find("h1, h2, h3, h4, h5, h6").Each(func(_ int, sel *goquery.Selection) {
		tagName := goquery.NodeName(sel)
		if len(tagName) != 2 || tagName[0] != 'h' {
			return
		}
		level := int(tagName[1] - '0')
		if lastLevel > 0 && level-lastLevel > 1 {
			issues++
		}
		lastLevel = level
	})
	return issues
}
