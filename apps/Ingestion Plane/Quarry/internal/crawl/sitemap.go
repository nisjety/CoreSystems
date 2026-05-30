package crawl

import (
	"context"
	"encoding/xml"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"
)

type sitemapURLSet struct {
	XMLName xml.Name     `xml:"urlset"`
	URLs    []sitemapURL `xml:"url"`
}

type sitemapURL struct {
	Loc string `xml:"loc"`
}

type sitemapIndex struct {
	XMLName  xml.Name       `xml:"sitemapindex"`
	Sitemaps []sitemapEntry `xml:"sitemap"`
}

type sitemapEntry struct {
	Loc string `xml:"loc"`
}

func FetchSitemapURLs(ctx context.Context, rawURL string) ([]string, error) {
	parsed, err := url.Parse(rawURL)
	if err != nil {
		return nil, err
	}
	sitemapURL := parsed.Scheme + "://" + parsed.Host + "/sitemap.xml"

	data, err := fetchURL(ctx, sitemapURL)
	if err != nil {
		return nil, err
	}

	var urlSet sitemapURLSet
	if err := xml.Unmarshal(data, &urlSet); err == nil && len(urlSet.URLs) > 0 {
		result := make([]string, 0, len(urlSet.URLs))
		for _, entry := range urlSet.URLs {
			canonical, canonicalErr := CanonicalizeURL(entry.Loc, nil, false)
			if canonicalErr == nil {
				result = append(result, canonical)
			}
		}
		return result, nil
	}

	var index sitemapIndex
	if err := xml.Unmarshal(data, &index); err == nil && len(index.Sitemaps) > 0 {
		result := make([]string, 0, 64)
		for _, entry := range index.Sitemaps {
			childData, childErr := fetchURL(ctx, entry.Loc)
			if childErr != nil {
				continue
			}
			var childSet sitemapURLSet
			if err := xml.Unmarshal(childData, &childSet); err != nil {
				continue
			}
			for _, child := range childSet.URLs {
				canonical, canonicalErr := CanonicalizeURL(child.Loc, nil, false)
				if canonicalErr == nil {
					result = append(result, canonical)
				}
			}
		}
		return result, nil
	}

	return nil, fmt.Errorf("sitemap did not contain usable urls")
}

func fetchURL(ctx context.Context, rawURL string) ([]byte, error) {
	reqCtx, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()

	req, err := http.NewRequestWithContext(reqCtx, http.MethodGet, rawURL, nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("User-Agent", "Quarry/1.0")

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("unexpected status %d", resp.StatusCode)
	}

	body, err := io.ReadAll(io.LimitReader(resp.Body, 5<<20))
	if err != nil {
		return nil, err
	}
	if strings.TrimSpace(string(body)) == "" {
		return nil, fmt.Errorf("empty response body")
	}
	return body, nil
}
