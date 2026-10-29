package pulse

import (
	"context"
	"errors"
	"math"
	"sort"
	"strings"
	"time"
)

const (
	SearchComparisonRowLimitDefault = 500
	SearchComparisonRowLimitMax     = 1000
	SearchComparisonDaysMax         = 90
)

type SearchComparisonRow struct {
	Key               string     `json:"key"`
	State             string     `json:"state"`
	Current           *SearchRow `json:"current,omitempty"`
	Previous          *SearchRow `json:"previous,omitempty"`
	ClicksChange      *int64     `json:"clicksChange,omitempty"`
	ImpressionsChange *int64     `json:"impressionsChange,omitempty"`
	PositionChange    *float64   `json:"positionChange,omitempty"`
	ClicksChangePct   *float64   `json:"clicksChangePercent,omitempty"`
}

type SearchComparisonProjection struct {
	Site              string                `json:"site"`
	Source            string                `json:"source"`
	Status            string                `json:"status"`
	Dimension         string                `json:"dimension"`
	CurrentPeriod     Period                `json:"currentPeriod"`
	PreviousPeriod    Period                `json:"previousPeriod"`
	Rows              []SearchComparisonRow `json:"rows"`
	RowLimit          int                   `json:"rowLimit"`
	RowsAtLimit       bool                  `json:"rowsAtLimit"`
	ComparableRows    int                   `json:"comparableRows"`
	UnmatchedRows     int                   `json:"unmatchedRows"`
	PropertyScope     string                `json:"propertyScope,omitempty"`
	ReportingProperty string                `json:"reportingProperty,omitempty"`
	UpdatedAt         string                `json:"updatedAt,omitempty"`
	Reason            string                `json:"reason,omitempty"`
}

// ComparisonPeriods accepts two adjacent, equal-length, completed windows of
// up to 90 days. An absent GSC row is deliberately not interpreted as zero.
func ComparisonPeriods(currentStart, currentEnd, previousStart, previousEnd string, now time.Time) (Period, Period, error) {
	current, err := parseComparisonPeriod(currentStart, currentEnd, now)
	if err != nil {
		return Period{}, Period{}, err
	}
	previous, err := parseComparisonPeriod(previousStart, previousEnd, now)
	if err != nil {
		return Period{}, Period{}, err
	}
	currentStartTime, _ := time.Parse(dateLayout, current.StartDate)
	currentEndTime, _ := time.Parse(dateLayout, current.EndDate)
	previousStartTime, _ := time.Parse(dateLayout, previous.StartDate)
	previousEndTime, _ := time.Parse(dateLayout, previous.EndDate)
	currentDays := int(currentEndTime.Sub(currentStartTime).Hours()/24) + 1
	previousDays := int(previousEndTime.Sub(previousStartTime).Hours()/24) + 1
	if currentDays != previousDays || currentDays > SearchComparisonDaysMax || !previousEndTime.AddDate(0, 0, 1).Equal(currentStartTime) {
		return Period{}, Period{}, errors.New("comparison periods must be adjacent, equal length, and no longer than 90 days")
	}
	return current, previous, nil
}

func parseComparisonPeriod(start, end string, now time.Time) (Period, error) {
	startTime, startErr := time.Parse(dateLayout, start)
	endTime, endErr := time.Parse(dateLayout, end)
	if startErr != nil || endErr != nil || endTime.Before(startTime) {
		return Period{}, errors.New("period dates must be valid YYYY-MM-DD dates in ascending order")
	}
	yesterday := time.Date(now.UTC().Year(), now.UTC().Month(), now.UTC().Day(), 0, 0, 0, 0, time.UTC).AddDate(0, 0, -1)
	if endTime.After(yesterday) {
		return Period{}, errors.New("period end date must be completed")
	}
	return Period{StartDate: startTime.Format(dateLayout), EndDate: endTime.Format(dateLayout)}, nil
}

func (s *Service) GetSearchComparison(ctx context.Context, requestedSite, dimension string, current, previous Period, rowLimit int) SearchComparisonProjection {
	now := s.now().UTC()
	site, siteErr := CanonicalSite(requestedSite)
	base := SearchComparisonProjection{Site: site, Source: "google_search_console", Status: StatusUnavailable, Dimension: dimension, CurrentPeriod: current, PreviousPeriod: previous, Rows: []SearchComparisonRow{}, RowLimit: rowLimit}
	if rowLimit < 1 || rowLimit > SearchComparisonRowLimitMax || (dimension != "query" && dimension != "page") {
		base.Reason = ReasonSourceUnavailable
		return base
	}
	if _, _, err := ComparisonPeriods(current.StartDate, current.EndDate, previous.StartDate, previous.EndDate, now); err != nil {
		base.Reason = ReasonSourceUnavailable
		return base
	}
	if siteErr != nil {
		base.Reason = ReasonPropertyUnverified
		return base
	}
	if !s.cfg.complete() || validateConfig(s.cfg) != nil {
		base.Reason = ReasonNotConfigured
		return base
	}
	ctx, cancel := context.WithTimeout(ctx, 12*time.Second)
	defer cancel()
	token, err := readBearerToken(s.cfg.IntegrationBearerTokenFile)
	if err != nil {
		base.Reason = ReasonSourceUnavailable
		return base
	}
	property, err := s.verifiedProperty(ctx, token, site)
	if err != nil {
		base.Reason = ReasonSourceUnavailable
		return base
	}
	if property == "" {
		base.Reason = ReasonPropertyUnverified
		return base
	}
	currentRows, previousRows, err := s.readComparisonRows(ctx, token, property, dimension, current, previous, rowLimit)
	if err != nil {
		base.Reason = ReasonSourceUnavailable
		return base
	}
	base.Rows = compareSearchRows(currentRows, previousRows)
	base.Status = StatusConnected
	base.RowsAtLimit = len(currentRows) == rowLimit || len(previousRows) == rowLimit
	for _, row := range base.Rows {
		if row.State == "compared" {
			base.ComparableRows++
		} else {
			base.UnmatchedRows++
		}
	}
	if strings.HasPrefix(property, "sc-domain:") {
		base.PropertyScope = "domain"
	} else {
		base.PropertyScope = "url_prefix"
	}
	base.ReportingProperty = property
	base.UpdatedAt = now.Format(time.RFC3339)
	return base
}

func compareSearchRows(currentRows, previousRows []SearchRow) []SearchComparisonRow {
	currentByKey := make(map[string]SearchRow, len(currentRows))
	previousByKey := make(map[string]SearchRow, len(previousRows))
	for _, row := range currentRows {
		currentByKey[row.Key] = row
	}
	for _, row := range previousRows {
		previousByKey[row.Key] = row
	}
	keys := make(map[string]struct{}, len(currentByKey)+len(previousByKey))
	for key := range currentByKey {
		keys[key] = struct{}{}
	}
	for key := range previousByKey {
		keys[key] = struct{}{}
	}
	rows := make([]SearchComparisonRow, 0, len(keys))
	for key := range keys {
		current, hasCurrent := currentByKey[key]
		previous, hasPrevious := previousByKey[key]
		row := SearchComparisonRow{Key: key, State: "not_comparable"}
		if hasCurrent {
			row.Current = &current
		}
		if hasPrevious {
			row.Previous = &previous
		}
		if hasCurrent && hasPrevious {
			row.State = "compared"
			clicksChange := current.Clicks - previous.Clicks
			impressionsChange := current.Impressions - previous.Impressions
			positionChange := current.Position - previous.Position
			row.ClicksChange = &clicksChange
			row.ImpressionsChange = &impressionsChange
			row.PositionChange = &positionChange
			if previous.Clicks > 0 {
				changePct := float64(clicksChange) / float64(previous.Clicks) * 100
				if !math.IsInf(changePct, 0) && !math.IsNaN(changePct) {
					row.ClicksChangePct = &changePct
				}
			}
		}
		rows = append(rows, row)
	}
	sort.Slice(rows, func(i, j int) bool {
		left, right := rows[i], rows[j]
		if left.State != right.State {
			return left.State == "compared"
		}
		if left.ClicksChange != nil && right.ClicksChange != nil && *left.ClicksChange != *right.ClicksChange {
			return *left.ClicksChange < *right.ClicksChange
		}
		return left.Key < right.Key
	})
	return rows
}

func (s *Service) readComparisonRows(ctx context.Context, token, property, dimension string, current, previous Period, rowLimit int) ([]SearchRow, []SearchRow, error) {
	type result struct {
		period Period
		rows   []SearchRow
		err    error
	}
	ctx, cancel := context.WithCancel(ctx)
	defer cancel()
	results := make(chan result, 2)
	for _, period := range []Period{current, previous} {
		period := period
		go func() {
			rows, err := s.readSearchRowsWithLimit(ctx, token, period, property, dimension, rowLimit)
			results <- result{period: period, rows: rows, err: err}
		}()
	}
	var currentRows, previousRows []SearchRow
	for i := 0; i < 2; i++ {
		select {
		case <-ctx.Done():
			return nil, nil, ctx.Err()
		case response := <-results:
			if response.err != nil {
				return nil, nil, response.err
			}
			if response.period == current {
				currentRows = response.rows
			} else {
				previousRows = response.rows
			}
		}
	}
	return currentRows, previousRows, nil
}
