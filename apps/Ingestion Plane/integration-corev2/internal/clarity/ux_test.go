package clarity

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestUXExportsPreserveMeasuredZeroAndOmitPrivateData(t *testing.T) {
	raw := `[{"metricName":"Traffic","information":[{"totalSessionCount":22}]},{"metricName":"DeadClickCount","information":[{"sessionsWithMetricPercentage":0,"privateUser":"private"}]},{"metricName":"ScrollDepth","information":[{"averageScrollDepth":30.27}]},{"metricName":"EngagementTime","information":[{"activeTime":37,"totalTime":59}]}]`
	got, err := parseTraffic([]byte(raw))
	if err != nil || got.UX == nil || got.UX.DeadClickPercent == nil || *got.UX.DeadClickPercent != 0 || *got.UX.AverageScrollDepth != 30.27 || *got.UX.ActiveSeconds != 37 || got.UX.RageClickPercent != nil {
		t.Fatalf("missing measured UX: %+v %v", got, err)
	}
	encoded, _ := json.Marshal(got)
	if strings.Contains(string(encoded), "private") {
		t.Fatal("private export data exposed")
	}
	copy := clone(Snapshot{Metrics: got})
	*copy.Metrics.UX.ActiveSeconds = 999
	if *got.UX.ActiveSeconds != 37 {
		t.Fatal("UX cache can be mutated by a reader")
	}
}

func TestUXRejectsInvalidPercentagesAndAmbiguousAggregates(t *testing.T) {
	traffic := `{"metricName":"Traffic","information":[{"totalSessionCount":1}]}`
	for _, ux := range []string{
		`{"metricName":"RageClickCount","information":[{"sessionsWithMetricPercentage":101}]}`,
		`{"metricName":"ScrollDepth","information":[{"averageScrollDepth":-1}]}`,
		`{"metricName":"EngagementTime","information":[{"activeTime":60,"totalTime":59}]}`,
		`{"metricName":"EngagementTime","information":[{"activeTime":"NaN"}]}`,
		`{"metricName":"DeadClickCount","information":[{},{}]}`,
		`{"metricName":"ScrollDepth","information":[{}]},{"metricName":"ScrollDepth","information":[{}]}`,
	} {
		if _, err := parseTraffic([]byte("[" + traffic + "," + ux + "]")); err == nil {
			t.Fatalf("invalid UX accepted: %s", ux)
		}
	}
}
