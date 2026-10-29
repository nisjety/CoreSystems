package pulse

import (
	"context"
	"encoding/json"
	"errors"
	"math"
	"math/big"
	"net/url"
	"sort"
	"strconv"
	"strings"
	"unicode"
	"unicode/utf8"
)

const maxSafeInteger = 1<<53 - 1

type providerSearchRow struct {
	Keys        []string    `json:"keys"`
	Clicks      json.Number `json:"clicks"`
	Impressions json.Number `json:"impressions"`
	CTR         *float64    `json:"ctr"`
	Position    *float64    `json:"position"`
}

func (s *Service) readSearchRows(ctx context.Context, token string, period Period, property, dimension string) ([]SearchRow, error) {
	return s.readSearchRowsWithLimit(ctx, token, period, property, dimension, SearchRowLimit)
}

func (s *Service) readSearchRowsWithLimit(ctx context.Context, token string, period Period, property, dimension string, rowLimit int) ([]SearchRow, error) {
	result, err := s.callAction(ctx, token, "gsc.search_analytics.query", map[string]any{"siteUrl": property}, map[string]any{
		"startDate": period.StartDate, "endDate": period.EndDate, "dimensions": []string{dimension}, "rowLimit": rowLimit, "dataState": "final",
	})
	if err != nil {
		return nil, err
	}
	var response struct {
		Rows []providerSearchRow `json:"rows"`
	}
	if err := json.Unmarshal(result, &response); err != nil {
		return nil, errors.New("invalid search analytics response")
	}
	if len(response.Rows) > rowLimit {
		return nil, errors.New("too many search analytics rows")
	}
	rows := make([]SearchRow, 0, len(response.Rows))
	seen := make(map[string]bool, len(response.Rows))
	for _, row := range response.Rows {
		parsed, err := normalizeSearchRow(row, property, dimension)
		if err != nil || seen[parsed.Key] {
			return nil, errors.New("invalid search analytics row")
		}
		seen[parsed.Key] = true
		rows = append(rows, parsed)
	}
	return rows, nil
}

func normalizeSearchRow(row providerSearchRow, property, dimension string) (SearchRow, error) {
	if len(row.Keys) != 1 || !validSearchKey(row.Keys[0], property, dimension) {
		return SearchRow{}, errors.New("invalid search row key")
	}
	clicks, clicksErr := wholeSafeSearchCount(row.Clicks)
	impressions, impressionsErr := wholeSafeSearchCount(row.Impressions)
	if clicksErr != nil || impressionsErr != nil || clicks > impressions || !boundedSearchFloat(row.CTR, 1) || !boundedSearchFloat(row.Position, math.MaxFloat64) || (impressions > 0 && *row.Position < 1) {
		return SearchRow{}, errors.New("invalid search row metric")
	}
	// Derive CTR from validated counts so serialization cannot amplify provider
	// rounding differences. Never infer missing counts or positions.
	ctr := float64(0)
	if impressions > 0 {
		ctr = float64(clicks) / float64(impressions)
	}
	return SearchRow{Key: row.Keys[0], Clicks: clicks, Impressions: impressions, CTR: ctr, Position: *row.Position}, nil
}

// Search Console documents counts as doubles. Accept exactly whole decimal or
// exponent representations without permitting fractional values rounded by a
// float conversion near JavaScript's safe-integer boundary. Bound the input
// and exponent before rational parsing so hostile numbers cannot expand work.
func wholeSafeSearchCount(value json.Number) (int64, error) {
	invalid := errors.New("invalid search count")
	if len(value) == 0 || len(value) > 64 {
		return 0, invalid
	}
	if index := strings.IndexAny(string(value), "eE"); index >= 0 {
		exponent, err := strconv.Atoi(string(value)[index+1:])
		if err != nil || exponent < -64 || exponent > 64 {
			return 0, invalid
		}
	}
	rational, ok := new(big.Rat).SetString(string(value))
	if !ok || !rational.IsInt() || !rational.Num().IsInt64() {
		return 0, invalid
	}
	parsed := rational.Num().Int64()
	if parsed < 0 || parsed > maxSafeInteger {
		return 0, invalid
	}
	return parsed, nil
}

func boundedSearchFloat(value *float64, maximum float64) bool {
	return value != nil && !math.IsNaN(*value) && !math.IsInf(*value, 0) && *value >= 0 && *value <= maximum
}

func validSearchKey(key, property, dimension string) bool {
	limit := 512
	if dimension == "page" {
		limit = 2048
	}
	if strings.TrimSpace(key) == "" || len(key) > limit || !utf8.ValidString(key) || strings.ContainsFunc(key, unicode.IsControl) {
		return false
	}
	if dimension == "query" {
		return true
	}
	page, err := url.Parse(key)
	if err != nil || (page.Scheme != "http" && page.Scheme != "https") || page.User != nil || page.Opaque != "" || page.Fragment != "" || page.Host != page.Hostname() || !validPublicHostname(strings.ToLower(page.Hostname())) || strings.ContainsAny(key, "\\ ") {
		return false
	}
	if strings.HasPrefix(property, "sc-domain:") {
		domain := strings.TrimPrefix(property, "sc-domain:")
		host := strings.ToLower(page.Hostname())
		return host == domain || strings.HasSuffix(host, "."+domain)
	}
	prefix, err := url.Parse(property)
	return err == nil && prefix.Scheme == page.Scheme && prefix.Host == strings.ToLower(page.Host)
}

func searchOpportunities(queries []SearchRow) []Opportunity {
	opportunities := make([]Opportunity, 0)
	for _, row := range queries {
		if row.Impressions < 100 || row.CTR >= 0.05 || row.Position <= 0 || row.Position > 20 {
			continue
		}
		typeName := "content_expansion"
		switch {
		case row.Position <= 3:
			if row.CTR >= 0.03 {
				continue
			}
			typeName = "snippet_review"
		case row.Position <= 10 && row.CTR < 0.03:
			typeName = "title_meta"
		case row.Position <= 10:
			typeName = "title_test"
		}
		opportunities = append(opportunities, Opportunity{SearchRow: row, Type: typeName})
	}
	sort.Slice(opportunities, func(i, j int) bool {
		if opportunities[i].Impressions != opportunities[j].Impressions {
			return opportunities[i].Impressions > opportunities[j].Impressions
		}
		return opportunities[i].Key < opportunities[j].Key
	})
	return opportunities
}
