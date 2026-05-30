package scraper

import (
	"context"
	"fmt"
	"math"
	"regexp"
	"strings"

	"github.com/PuerkitoBio/goquery"

	"github.com/triodelab/quarry/internal/transform"
)

// KnowledgeArticle is the structured output of the knowledge_article extraction format.
// Designed for Zendesk/Intercom/Notion knowledge base ingestion.
type KnowledgeArticle struct {
	URL           string   `json:"url"`
	Title         string   `json:"title"`
	Summary       string   `json:"summary"`
	Body          string   `json:"body"` // markdown
	Author        string   `json:"author,omitempty"`
	PublishedDate string   `json:"published_date,omitempty"`
	Tags          []string `json:"tags"`
	ContentType   string   `json:"content_type"` // "article" | "faq" | "guide" | "reference"
	WordCount     int      `json:"word_count"`
	ReadingTime   string   `json:"reading_time"` // e.g. "3 min read"
	Language      string   `json:"language,omitempty"`
}

var howToRe = regexp.MustCompile(`(?i)\b(how to|step \d+|getting started|tutorial|guide)\b`)

// extractKnowledgeArticle extracts a structured knowledge article from HTML.
// Uses heuristic extraction (no AI required); suitable for high-throughput crawls.
func extractKnowledgeArticle(_ context.Context, targetURL, html string) KnowledgeArticle {
	article := KnowledgeArticle{
		URL:  targetURL,
		Tags: []string{},
	}

	if strings.TrimSpace(html) == "" {
		return article
	}

	doc, err := goquery.NewDocumentFromReader(strings.NewReader(html))
	if err != nil {
		return article
	}

	article.Title = extractArticleTitle(doc)
	article.Author = extractArticleAuthor(doc)
	article.PublishedDate = extractArticleDate(doc)
	article.Tags = extractArticleTags(doc)
	article.Language = strings.TrimSpace(doc.Find("html").AttrOr("lang", ""))
	article.Summary = extractArticleSummary(doc)
	article.Body = extractArticleBody(targetURL, html, doc)
	article.ContentType = detectArticleContentType(doc)

	article.WordCount = len(strings.Fields(article.Body))
	minRead := int(math.Ceil(float64(article.WordCount) / 200.0))
	if minRead < 1 {
		minRead = 1
	}
	article.ReadingTime = fmt.Sprintf("%d min read", minRead)

	return article
}

func extractArticleTitle(doc *goquery.Document) string {
	if og := doc.Find(`meta[property="og:title"]`).AttrOr("content", ""); og != "" {
		return strings.TrimSpace(og)
	}
	if h1 := strings.TrimSpace(doc.Find("h1").First().Text()); h1 != "" {
		return h1
	}
	return strings.TrimSpace(doc.Find("title").Text())
}

func extractArticleAuthor(doc *goquery.Document) string {
	if a := doc.Find(`meta[name="author"]`).AttrOr("content", ""); a != "" {
		return strings.TrimSpace(a)
	}
	bylineSelectors := []string{
		`.author`, `.byline`, `[itemprop="author"]`,
		`.post-author`, `.article-author`, `[rel="author"]`,
	}
	for _, sel := range bylineSelectors {
		if t := strings.TrimSpace(doc.Find(sel).First().Text()); t != "" {
			return t
		}
	}
	return ""
}

func extractArticleDate(doc *goquery.Document) string {
	metaSelectors := []string{
		`meta[property="article:published_time"]`,
		`meta[name="date"]`,
		`meta[name="pubdate"]`,
		`meta[property="og:updated_time"]`,
		`meta[name="DC.date.issued"]`,
	}
	for _, sel := range metaSelectors {
		if v := doc.Find(sel).AttrOr("content", ""); v != "" {
			return strings.TrimSpace(v)
		}
	}
	if dt := doc.Find("time[datetime]").AttrOr("datetime", ""); dt != "" {
		return strings.TrimSpace(dt)
	}
	return ""
}

func extractArticleTags(doc *goquery.Document) []string {
	seen := map[string]bool{}
	var tags []string

	add := func(t string) {
		t = strings.TrimSpace(t)
		if t == "" || seen[t] {
			return
		}
		seen[t] = true
		tags = append(tags, t)
	}

	doc.Find(`meta[property="article:tag"]`).Each(func(_ int, s *goquery.Selection) {
		add(s.AttrOr("content", ""))
	})
	if kw := doc.Find(`meta[name="keywords"]`).AttrOr("content", ""); kw != "" {
		for _, k := range strings.Split(kw, ",") {
			add(k)
		}
	}
	doc.Find("a.tag, a.category, .tags a, .categories a, .label a").Each(func(_ int, s *goquery.Selection) {
		add(s.Text())
	})

	if tags == nil {
		return []string{}
	}
	return tags
}

func extractArticleSummary(doc *goquery.Document) string {
	if desc := doc.Find(`meta[property="og:description"]`).AttrOr("content", ""); desc != "" {
		return strings.TrimSpace(desc)
	}
	if desc := doc.Find(`meta[name="description"]`).AttrOr("content", ""); desc != "" {
		return strings.TrimSpace(desc)
	}
	// Fall back to first substantive paragraph
	for _, sel := range []string{"article p", "main p", ".content p", ".entry-content p", "p"} {
		summary := ""
		doc.Find(sel).EachWithBreak(func(_ int, s *goquery.Selection) bool {
			t := strings.TrimSpace(s.Text())
			if len(t) >= 40 {
				if len(t) > 300 {
					t = t[:297] + "..."
				}
				summary = t
				return false
			}
			return true
		})
		if summary != "" {
			return summary
		}
	}
	return ""
}

func extractArticleBody(targetURL, html string, doc *goquery.Document) string {
	// Try to scope to main content container to reduce noise
	contentSelectors := []string{
		"article",
		"main",
		`[role="main"]`,
		".article-body",
		".post-content",
		".entry-content",
		".prose",
		".content",
	}
	for _, sel := range contentSelectors {
		if s, htmlErr := doc.Find(sel).First().Html(); htmlErr == nil && strings.TrimSpace(s) != "" {
			if md, mdErr := transform.HTMLToMarkdown(targetURL, s, true); mdErr == nil {
				return md
			}
		}
	}
	// Fallback: full page markdown
	if md, mdErr := transform.HTMLToMarkdown(targetURL, html, true); mdErr == nil {
		return md
	}
	return ""
}

// detectArticleContentType classifies a page as faq, reference, guide, or article.
func detectArticleContentType(doc *goquery.Document) string {
	// FAQ: ≥3 headings ending with "?"
	faqCount := 0
	doc.Find("h2, h3").Each(func(_ int, s *goquery.Selection) {
		if strings.HasSuffix(strings.TrimRight(strings.ToLower(s.Text()), " \t"), "?") {
			faqCount++
		}
	})
	if faqCount >= 3 {
		return "faq"
	}

	// Reference: multiple code blocks
	if doc.Find("pre code, pre.highlight, .code-block, .highlight").Length() >= 3 {
		return "reference"
	}

	// Guide / how-to: numbered list or keyword in h1
	h1 := doc.Find("h1").First().Text()
	if howToRe.MatchString(h1) || doc.Find("ol li").Length() >= 4 {
		return "guide"
	}

	return "article"
}
