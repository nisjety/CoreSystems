package scraper

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"
	"time"
)

// StructuredExtractOptions controls single-page structured extraction.
type StructuredExtractOptions struct {
	Schema   string
	Prompt   string
	MaxAgeMs int64
}

// ExtractStructured fetches a page once, then runs the smart extraction pipeline
// on the captured HTML. Keeping fetch + extract together avoids duplicated page
// loads and keeps cache behavior consistent across sync and async API paths.
func (s *Scraper) ExtractStructured(ctx context.Context, targetURL string, opts *StructuredExtractOptions) (map[string]interface{}, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if s == nil || s.extractor == nil {
		return nil, fmt.Errorf("structured extractor is not initialized")
	}
	if strings.TrimSpace(targetURL) == "" {
		return nil, fmt.Errorf("url is required")
	}

	if opts == nil {
		opts = &StructuredExtractOptions{}
	}
	if strings.TrimSpace(opts.Schema) == "" && strings.TrimSpace(opts.Prompt) == "" {
		return nil, fmt.Errorf("schema or prompt is required")
	}

	html, cached, err := s.ScrapeCachedPage(ctx, targetURL, opts.MaxAgeMs)
	if err != nil {
		return nil, fmt.Errorf("fetch page: %w", err)
	}

	extraction, err := s.extractor.ExtractSmart(ctx, html, targetURL, opts.Schema, opts.Prompt)
	if err != nil {
		return nil, fmt.Errorf("extract structured data: %w", err)
	}

	payload, err := json.Marshal(extraction)
	if err != nil {
		return nil, fmt.Errorf("marshal extraction result: %w", err)
	}

	var result map[string]interface{}
	if err := json.Unmarshal(payload, &result); err != nil {
		return nil, fmt.Errorf("decode extraction result: %w", err)
	}

	result["source_url"] = targetURL
	result["cached"] = cached
	result["extracted_at"] = time.Now().UTC().Format(time.RFC3339)
	return result, nil
}
