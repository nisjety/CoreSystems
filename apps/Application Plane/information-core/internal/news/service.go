package news

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/xml"
	"errors"
	"fmt"
	"io"
	"net/http"
	"regexp"
	"slices"
	"strings"
	"time"

	"coresystem/apps/application-plane/information-core/internal/cache"
	"coresystem/apps/application-plane/information-core/internal/config"
)

type Service struct {
	client *http.Client
	cache  *cache.Store
}

type Article struct {
	ID          string   `json:"id"`
	Title       string   `json:"title"`
	Description string   `json:"description"`
	Link        string   `json:"link"`
	Source      string   `json:"source"`
	PublishDate string   `json:"publishDate"`
	Image       string   `json:"image,omitempty"`
	Categories  []string `json:"categories"`
}

type Response struct {
	Articles    []Article `json:"articles"`
	LastUpdated string    `json:"lastUpdated"`
	TotalCount  int       `json:"totalCount"`
	HasMore     bool      `json:"hasMore"`
}

type rssFeed struct {
	Name     string
	URL      string
	Category string
}

type rssDocument struct {
	Channel struct {
		Items []rssItem `xml:"item"`
	} `xml:"channel"`
}

type rssItem struct {
	Title       string      `xml:"title"`
	Link        string      `xml:"link"`
	Description string      `xml:"description"`
	Content     string      `xml:"encoded"`
	PubDate     string      `xml:"pubDate"`
	Categories  []string    `xml:"category"`
	Enclosure   *enclosure  `xml:"enclosure"`
	Media       []mediaNode `xml:"content"`
	Thumbnail   []mediaNode `xml:"thumbnail"`
}

type enclosure struct {
	URL  string `xml:"url,attr"`
	Type string `xml:"type,attr"`
}

type mediaNode struct {
	URL    string `xml:"url,attr"`
	Medium string `xml:"medium,attr"`
}

var (
	tagRe = regexp.MustCompile(`<[^>]+>`)
	imgRe = regexp.MustCompile(`(?i)<img[^>]+src=["']([^"']+)["']`)
)

var feeds = []rssFeed{
	{Name: "VG Nyheter", URL: "https://www.vg.no/rss/feed/", Category: "General"},
	{Name: "Aftenposten", URL: "https://www.aftenposten.no/rss", Category: "General"},
	{Name: "NRK Nyheter", URL: "https://www.nrk.no/toppsaker.rss", Category: "General"},
	{Name: "Dagbladet", URL: "https://www.dagbladet.no/rss", Category: "General"},
	{Name: "E24", URL: "https://e24.no/rss", Category: "Business"},
	{Name: "DN", URL: "https://www.dn.no/rss", Category: "Business"},
	{Name: "Tek.no", URL: "https://www.tek.no/rss/feed/", Category: "Technology"},
	{Name: "Digi.no", URL: "https://www.digi.no/rss", Category: "Technology"},
}

func NewService(client *http.Client, cacheStore *cache.Store) *Service {
	return &Service{client: client, cache: cacheStore}
}

func (s *Service) Latest(ctx context.Context, userAgent, category string, limit, offset int, maxAgeHours int) (Response, error) {
	key := fmt.Sprintf("news:%s:%d:%d:%d", strings.ToLower(strings.TrimSpace(category)), limit, offset, maxAgeHours)
	if cached, ok := s.cache.Get(key); ok {
		if payload, ok := cached.(Response); ok {
			return payload, nil
		}
	}

	selectedFeeds := feeds
	if category = strings.TrimSpace(category); category != "" {
		filtered := make([]rssFeed, 0, len(feeds))
		for _, feed := range feeds {
			if strings.EqualFold(feed.Category, category) {
				filtered = append(filtered, feed)
			}
		}
		selectedFeeds = filtered
	}

	articles := make([]Article, 0, limit*2)
	for _, feed := range selectedFeeds {
		items, err := s.fetchFeed(ctx, userAgent, feed)
		if err != nil {
			continue
		}
		articles = append(articles, items...)
	}

	if len(articles) == 0 {
		return Response{}, errors.New("no news available")
	}

	slices.SortFunc(articles, func(a, b Article) int {
		return strings.Compare(b.PublishDate, a.PublishDate)
	})

	articles = dedupe(articles)
	if maxAgeHours > 0 {
		cutoff := time.Now().Add(-time.Duration(maxAgeHours) * time.Hour)
		filtered := articles[:0]
		for _, article := range articles {
			parsed, err := time.Parse(time.RFC3339, article.PublishDate)
			if err == nil && parsed.Before(cutoff) {
				continue
			}
			filtered = append(filtered, article)
		}
		articles = filtered
	}

	totalCount := len(articles)
	if offset > totalCount {
		offset = totalCount
	}
	end := min(totalCount, offset+limit)
	result := Response{
		Articles:    articles[offset:end],
		LastUpdated: time.Now().UTC().Format(time.RFC3339),
		TotalCount:  totalCount,
		HasMore:     end < totalCount,
	}
	s.cache.Set(key, config.TTL(300), result)
	return result, nil
}

func (s *Service) fetchFeed(ctx context.Context, userAgent string, feed rssFeed) ([]Article, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, feed.URL, nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("User-Agent", userAgent)

	resp, err := s.client.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("rss upstream status %d", resp.StatusCode)
	}

	body, err := io.ReadAll(io.LimitReader(resp.Body, 2<<20))
	if err != nil {
		return nil, err
	}

	var document rssDocument
	if err := xml.Unmarshal(body, &document); err != nil {
		return nil, err
	}

	items := make([]Article, 0, min(12, len(document.Channel.Items)))
	for _, item := range document.Channel.Items[:min(12, len(document.Channel.Items))] {
		link := strings.TrimSpace(item.Link)
		if link == "" {
			continue
		}
		publishedAt := normalizePubDate(item.PubDate)
		categories := append([]string{feed.Category}, cleanCategories(item.Categories)...)
		items = append(items, Article{
			ID:          stableID(link),
			Title:       fallback(strings.TrimSpace(item.Title), "Untitled article"),
			Description: cleanText(firstNonEmpty(item.Description, item.Content)),
			Link:        link,
			Source:      feed.Name,
			PublishDate: publishedAt,
			Image:       extractImage(item),
			Categories:  categories,
		})
	}

	return items, nil
}

func stableID(link string) string {
	sum := sha256.Sum256([]byte(link))
	return hex.EncodeToString(sum[:8])
}

func normalizePubDate(value string) string {
	value = strings.TrimSpace(value)
	if value == "" {
		return time.Now().UTC().Format(time.RFC3339)
	}
	layouts := []string{time.RFC1123Z, time.RFC1123, time.RFC822Z, time.RFC822, time.RFC3339}
	for _, layout := range layouts {
		parsed, err := time.Parse(layout, value)
		if err == nil {
			return parsed.UTC().Format(time.RFC3339)
		}
	}
	return time.Now().UTC().Format(time.RFC3339)
}

func cleanText(input string) string {
	text := strings.TrimSpace(tagRe.ReplaceAllString(input, " "))
	text = strings.Join(strings.Fields(text), " ")
	if len(text) > 200 {
		return text[:200] + "..."
	}
	return text
}

func cleanCategories(values []string) []string {
	seen := make(map[string]struct{}, len(values))
	out := make([]string, 0, len(values))
	for _, value := range values {
		cleaned := strings.TrimSpace(value)
		if cleaned == "" {
			continue
		}
		if _, ok := seen[cleaned]; ok {
			continue
		}
		seen[cleaned] = struct{}{}
		out = append(out, cleaned)
	}
	return out
}

func extractImage(item rssItem) string {
	for _, media := range item.Media {
		if strings.TrimSpace(media.URL) != "" && (media.Medium == "" || strings.EqualFold(media.Medium, "image")) {
			return media.URL
		}
	}
	for _, thumb := range item.Thumbnail {
		if strings.TrimSpace(thumb.URL) != "" {
			return thumb.URL
		}
	}
	if item.Enclosure != nil && strings.Contains(strings.ToLower(item.Enclosure.Type), "image") {
		return item.Enclosure.URL
	}
	if match := imgRe.FindStringSubmatch(firstNonEmpty(item.Content, item.Description)); len(match) == 2 {
		return match[1]
	}
	return ""
}

func dedupe(items []Article) []Article {
	seen := make(map[string]struct{}, len(items))
	out := make([]Article, 0, len(items))
	for _, item := range items {
		key := strings.ToLower(strings.TrimSpace(item.Title + "|" + item.Source))
		if _, ok := seen[key]; ok {
			continue
		}
		seen[key] = struct{}{}
		out = append(out, item)
	}
	return out
}

func firstNonEmpty(values ...string) string {
	for _, value := range values {
		if strings.TrimSpace(value) != "" {
			return value
		}
	}
	return ""
}

func fallback(value, other string) string {
	if value == "" {
		return other
	}
	return value
}

func min(a, b int) int {
	if a < b {
		return a
	}
	return b
}
