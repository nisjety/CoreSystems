package pulse

import (
	"errors"
	"math"
	"regexp"
	"sort"
	"strconv"
	"time"
)

var ga4MetricNames = []string{"totalUsers", "sessions", "engagedSessions", "screenPageViews", "keyEvents", "userEngagementDuration"}
var ga4Decimal = regexp.MustCompile(`^(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]{1,3})?$`)
var ga4Count = regexp.MustCompile(`^(?:0|[1-9][0-9]{0,15})$`)
var errGA4Report = errors.New("invalid GA4 report")

type ga4Header struct {
	Name string `json:"name"`
}
type ga4Value struct {
	Value string `json:"value"`
}
type ga4Row struct {
	Dimensions []ga4Value `json:"dimensionValues"`
	Metrics    []ga4Value `json:"metricValues"`
}
type ga4Report struct {
	Dimensions []ga4Header `json:"dimensionHeaders"`
	Metrics    []ga4Header `json:"metricHeaders"`
	Rows       []ga4Row    `json:"rows"`
	RowCount   *int64      `json:"rowCount"`
	Metadata   struct {
		TimeZone    string     `json:"timeZone"`
		Thresholded bool       `json:"subjectToThresholding"`
		DataLoss    bool       `json:"dataLossFromOtherRow"`
		Samples     []struct{} `json:"samplingMetadatas"`
		EmptyReason string     `json:"emptyReason"`
	} `json:"metadata"`
}

func normalizeGA4Reports(summary, daily ga4Report, period Period) (*GA4Metrics, []GA4Day, string, *GA4Quality, error) {
	bad := func() (*GA4Metrics, []GA4Day, string, *GA4Quality, error) { return nil, nil, "", nil, errGA4Report }
	if !validGA4Zone(summary.Metadata.TimeZone) || summary.Metadata.TimeZone != daily.Metadata.TimeZone || len(summary.Dimensions) != 0 || len(summary.Rows) > 1 || len(daily.Dimensions) != 1 || daily.Dimensions[0].Name != "date" || len(daily.Rows) > 100 {
		return bad()
	}
	quality := GA4Quality{Thresholded: summary.Metadata.Thresholded || daily.Metadata.Thresholded, Sampled: len(summary.Metadata.Samples) > 0 || len(daily.Metadata.Samples) > 0, DataLoss: summary.Metadata.DataLoss || daily.Metadata.DataLoss}
	var flags *GA4Quality
	if quality.Thresholded || quality.Sampled || quality.DataLoss {
		flags = &quality
	}
	// GA4 omits summary headers, rows, and rowCount entirely for a genuinely
	// empty property. Daily still identifies its requested metric schema, so
	// require that schema and reject any daily data before exposing no totals.
	if len(summary.Metrics) == 0 && len(summary.Rows) == 0 && summary.RowCount == nil {
		if len(daily.Rows) != 0 || (daily.RowCount != nil && *daily.RowCount != 0) {
			return bad()
		}
		if _, err := ga4HeaderOrder(daily.Metrics); err != nil {
			return bad()
		}
		return nil, []GA4Day{}, summary.Metadata.TimeZone, flags, nil
	}
	for _, report := range []ga4Report{summary, daily} {
		if report.RowCount != nil && *report.RowCount != int64(len(report.Rows)) {
			return bad()
		}
		if _, err := ga4HeaderOrder(report.Metrics); err != nil {
			return bad()
		}
	}
	days := make([]GA4Day, 0, len(daily.Rows))
	seen := map[string]bool{}
	for _, row := range daily.Rows {
		if len(row.Dimensions) != 1 {
			return bad()
		}
		date, err := time.Parse("20060102", row.Dimensions[0].Value)
		if err != nil || date.Format("20060102") != row.Dimensions[0].Value {
			return bad()
		}
		key := date.Format(dateLayout)
		if key < period.StartDate || key > period.EndDate || seen[key] {
			return bad()
		}
		values, err := ga4MetricValues(daily.Metrics, row.Metrics)
		if err != nil {
			return bad()
		}
		seen[key] = true
		days = append(days, GA4Day{Date: key, GA4Metrics: values})
	}
	sort.Slice(days, func(i, j int) bool { return days[i].Date < days[j].Date })
	// A missing summary is not a measured zero, particularly under privacy
	// thresholds or emptyReason. Keep the entire public report explicitly empty.
	if len(summary.Rows) == 0 {
		return nil, []GA4Day{}, summary.Metadata.TimeZone, flags, nil
	}
	if len(summary.Rows[0].Dimensions) != 0 || summary.Metadata.EmptyReason != "" || daily.Metadata.EmptyReason != "" {
		return bad()
	}
	totals, err := ga4MetricValues(summary.Metrics, summary.Rows[0].Metrics)
	if err != nil {
		return bad()
	}
	return &totals, days, summary.Metadata.TimeZone, flags, nil
}
func ga4HeaderOrder(headers []ga4Header) (map[string]int, error) {
	if len(headers) != len(ga4MetricNames) {
		return nil, errGA4Report
	}
	indexes := map[string]int{}
	for i, header := range headers {
		known := false
		for _, name := range ga4MetricNames {
			if header.Name == name {
				known = true
				break
			}
		}
		if !known {
			return nil, errGA4Report
		}
		if _, duplicate := indexes[header.Name]; duplicate {
			return nil, errGA4Report
		}
		indexes[header.Name] = i
	}
	return indexes, nil
}
func ga4MetricValues(headers []ga4Header, values []ga4Value) (GA4Metrics, error) {
	indexes, err := ga4HeaderOrder(headers)
	if err != nil || len(values) != len(headers) {
		return GA4Metrics{}, errGA4Report
	}
	counts := make([]int64, 4)
	for i, name := range ga4MetricNames[:4] {
		value := values[indexes[name]].Value
		if !ga4Count.MatchString(value) {
			return GA4Metrics{}, errGA4Report
		}
		counts[i], err = strconv.ParseInt(value, 10, 64)
		if err != nil || counts[i] > maxSafeInteger {
			return GA4Metrics{}, errGA4Report
		}
	}
	fractions := make([]float64, 2)
	for i, name := range ga4MetricNames[4:] {
		value := values[indexes[name]].Value
		if len(value) > 64 || !ga4Decimal.MatchString(value) {
			return GA4Metrics{}, errGA4Report
		}
		fractions[i], err = strconv.ParseFloat(value, 64)
		if err != nil || math.IsNaN(fractions[i]) || math.IsInf(fractions[i], 0) || fractions[i] < 0 {
			return GA4Metrics{}, errGA4Report
		}
	}
	return GA4Metrics{Users: counts[0], Sessions: counts[1], EngagedSessions: counts[2], PageViews: counts[3], KeyEvents: fractions[0], EngagementSeconds: fractions[1]}, nil
}
