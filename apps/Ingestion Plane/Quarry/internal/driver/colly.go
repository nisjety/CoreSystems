package driver

import (
	"context"
	"crypto/tls"
	"fmt"
	"net/http"
	"net/url"
	"strings"
	"sync"
	"time"

	"github.com/gocolly/colly/v2"
)

type CollyDriver struct {
	userAgent       string
	mu              sync.RWMutex
	lastHTML        string
	lastURL         string
	lastLinks       []string
	lastCode        int
	lastContentType string
}

func NewCollyDriver(userAgent string) *CollyDriver {
	return &CollyDriver{userAgent: userAgent}
}

func (d *CollyDriver) Name() string { return "colly" }

func (d *CollyDriver) Fetch(ctx context.Context, targetURL string, opts *FetchOptions) (*FetchResult, error) {
	collector := colly.NewCollector(
		colly.UserAgent(d.userAgent),
	)
	collector.SetRequestTimeout(30 * time.Second)

	links := make(map[string]struct{})
	var htmlBuilder strings.Builder
	statusCode := http.StatusOK
	contentType := "text/html"

	if opts != nil && len(opts.Headers) > 0 {
		collector.OnRequest(func(r *colly.Request) {
			for k, v := range opts.Headers {
				r.Headers.Set(k, v)
			}
		})
	}

	collector.OnResponse(func(r *colly.Response) {
		statusCode = r.StatusCode
		contentType = strings.TrimSpace(r.Headers.Get("Content-Type"))
		if contentType == "" {
			contentType = "text/html"
		}
		htmlBuilder.Write(r.Body)
	})

	collector.OnHTML("a[href]", func(e *colly.HTMLElement) {
		href := strings.TrimSpace(e.Attr("href"))
		if href == "" {
			return
		}
		absolute := e.Request.AbsoluteURL(href)
		if absolute != "" {
			links[absolute] = struct{}{}
		}
	})

	collector.WithTransport(&http.Transport{
		Proxy: http.ProxyFromEnvironment,
		TLSClientConfig: &tls.Config{
			InsecureSkipVerify: true, // Skip TLS verification for development/testing
		},
	})

	visitErrCh := make(chan error, 1)
	go func() {
		visitErrCh <- collector.Visit(targetURL)
	}()

	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	case err := <-visitErrCh:
		if err != nil {
			return nil, fmt.Errorf("colly visit failed: %w", err)
		}
	}

	collectedLinks := make([]string, 0, len(links))
	for item := range links {
		collectedLinks = append(collectedLinks, item)
	}

	html := htmlBuilder.String()
	result := &FetchResult{
		URL:         targetURL,
		Status:      statusCode,
		ContentType: contentType,
		HTML:        html,
		RawHTML:     html,
		Links:       collectedLinks,
		Rendered:    false,
	}

	d.mu.Lock()
	d.lastHTML = html
	d.lastURL = targetURL
	d.lastLinks = collectedLinks
	d.lastCode = statusCode
	d.lastContentType = contentType
	d.mu.Unlock()

	return result, nil
}

func (d *CollyDriver) HTML(ctx context.Context) (string, error) {
	select {
	case <-ctx.Done():
		return "", ctx.Err()
	default:
	}
	d.mu.RLock()
	defer d.mu.RUnlock()
	return d.lastHTML, nil
}

func (d *CollyDriver) Click(ctx context.Context, selector string) error {
	_ = ctx
	_ = selector
	return fmt.Errorf("click is not supported by colly driver")
}

func (d *CollyDriver) Type(ctx context.Context, selector, text string) error {
	_ = ctx
	_ = selector
	_ = text
	return fmt.Errorf("type is not supported by colly driver")
}

func (d *CollyDriver) Press(ctx context.Context, key string) error {
	_ = ctx
	_ = key
	return fmt.Errorf("press is not supported by colly driver")
}

func (d *CollyDriver) Wait(ctx context.Context, milliseconds int) error {
	if milliseconds <= 0 {
		return nil
	}
	t := time.NewTimer(time.Duration(milliseconds) * time.Millisecond)
	defer t.Stop()
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-t.C:
		return nil
	}
}

func (d *CollyDriver) Scroll(ctx context.Context, direction string) error {
	_ = ctx
	_ = direction
	return fmt.Errorf("scroll is not supported by colly driver")
}

func (d *CollyDriver) Screenshot(ctx context.Context, fullPage bool) ([]byte, error) {
	_ = ctx
	_ = fullPage
	return nil, fmt.Errorf("screenshot is not supported by colly driver")
}

func (d *CollyDriver) EvalJS(ctx context.Context, script string) (interface{}, error) {
	_ = ctx
	_ = script
	return nil, fmt.Errorf("executeJavascript is not supported by colly driver")
}

func (d *CollyDriver) GeneratePDF(ctx context.Context) ([]byte, error) {
	_ = ctx
	return nil, fmt.Errorf("generatePDF is not supported by colly driver")
}

func (d *CollyDriver) Close() error { return nil }

func normalizeURL(baseURL string, links map[string]struct{}) []string {
	if len(links) == 0 {
		return []string{}
	}
	base, err := url.Parse(baseURL)
	if err != nil {
		result := make([]string, 0, len(links))
		for l := range links {
			result = append(result, l)
		}
		return result
	}
	result := make([]string, 0, len(links))
	for l := range links {
		u, parseErr := url.Parse(l)
		if parseErr != nil {
			continue
		}
		if !u.IsAbs() {
			u = base.ResolveReference(u)
		}
		result = append(result, u.String())
	}
	return result
}
