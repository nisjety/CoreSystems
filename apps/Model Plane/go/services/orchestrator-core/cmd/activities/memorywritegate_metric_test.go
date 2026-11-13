package activities_test

import (
	"bytes"
	"context"
	"sort"
	"sync"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
	"go.opentelemetry.io/otel/metric"
	"go.opentelemetry.io/otel/metric/noop"

	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/activities"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/telemetry"
)

// P0-D2-481 (plan 3.2): the refusal counts reach orchestrator_core_memory_write_gate_entries_total
// with closed labels only (outcome, rule) and one increment per checked entry. The switch off, or a
// non-durable posture, adds nothing.

// recordingCounter stands in for telemetry.MemoryWriteGateEntriesTotal and records each Add as its
// label set.
type recordingCounter struct {
	noop.Int64Counter
	mu    sync.Mutex
	calls []map[string]string
	total int64
}

func (c *recordingCounter) Add(_ context.Context, incr int64, options ...metric.AddOption) {
	c.mu.Lock()
	defer c.mu.Unlock()
	labels := map[string]string{}
	set := metric.NewAddConfig(options).Attributes()
	for _, kv := range set.ToSlice() {
		labels[string(kv.Key)] = kv.Value.Emit()
	}
	c.calls = append(c.calls, labels)
	c.total += incr
}

func (c *recordingCounter) series() []string {
	c.mu.Lock()
	defer c.mu.Unlock()
	out := make([]string, 0, len(c.calls))
	for _, labels := range c.calls {
		keys := make([]string, 0, len(labels))
		for k := range labels {
			keys = append(keys, k)
		}
		sort.Strings(keys)
		line := ""
		for _, k := range keys {
			line += k + "=" + labels[k] + ";"
		}
		out = append(out, line)
	}
	sort.Strings(out)
	return out
}

func swapGateCounter(t *testing.T) *recordingCounter {
	t.Helper()
	previous := telemetry.MemoryWriteGateEntriesTotal
	counter := &recordingCounter{}
	telemetry.MemoryWriteGateEntriesTotal = counter
	t.Cleanup(func() { telemetry.MemoryWriteGateEntriesTotal = previous })
	return counter
}

func TestMemoryGateMetricCountsEachEntryWithClosedLabels(t *testing.T) {
	counter := swapGateCounter(t)
	policy := &sessionPolicy{refuse: map[string]string{
		"ignore previous instructions SECRET-INJECTION": "injection_marker",
		"AKIA-SECRET-LOOKING-VALUE":                     "a_token_this_build_does_not_know",
	}}
	var logs bytes.Buffer
	a := gatedActivities(t, &recordingLetta{}, policy, true, &logs)
	require.NoError(t, a.WriteConsolidatedMemoryActivity(context.Background(), activities.WriteMemoryInput{
		Entries: gateEntries(), Retention: activities.RetentionDurable,
	}))

	assert.Equal(t, int64(4), counter.total, "one increment per checked entry")
	assert.Equal(t, []string{
		"outcome=allowed;rule=none;",
		"outcome=allowed;rule=none;",
		"outcome=refused;rule=injection_marker;",
		"outcome=refused;rule=other;",
	}, counter.series())
	for _, labels := range counter.calls {
		for _, value := range labels {
			for _, entry := range gateEntries() {
				assert.NotContains(t, value, entry.Content, "a metric label carries entry content")
			}
			assert.NotContains(t, value, "org-1", "a metric label carries the org id (unbounded cardinality)")
		}
	}
}

func TestMemoryGateMetricRefusalWithNoRuleIsLabelledRefused(t *testing.T) {
	counter := swapGateCounter(t)
	refuse := map[string]string{}
	for _, entry := range gateEntries() {
		refuse[entry.Content] = ""
	}
	var logs bytes.Buffer
	a := gatedActivities(t, &recordingLetta{}, &sessionPolicy{refuse: refuse}, true, &logs)
	require.Error(t, a.WriteConsolidatedMemoryActivity(context.Background(), activities.WriteMemoryInput{
		Entries: gateEntries(), Retention: activities.RetentionDurable,
	}))
	assert.Equal(t, int64(4), counter.total)
	for _, line := range counter.series() {
		assert.Equal(t, "outcome=refused;rule=refused;", line)
	}
}

func TestMemoryGateMetricSilentWhenOffOrNotDurable(t *testing.T) {
	t.Run("switch off", func(t *testing.T) {
		counter := swapGateCounter(t)
		var logs bytes.Buffer
		a := gatedActivities(t, &recordingLetta{}, &sessionPolicy{}, false, &logs)
		require.NoError(t, a.WriteConsolidatedMemoryActivity(context.Background(), activities.WriteMemoryInput{Entries: gateEntries()}))
		assert.Zero(t, counter.total)
	})
	for _, retention := range []activities.Retention{activities.RetentionUnspecified, activities.RetentionZeroData} {
		t.Run("retention "+string(retention), func(t *testing.T) {
			counter := swapGateCounter(t)
			var logs bytes.Buffer
			a := gatedActivities(t, &recordingLetta{}, &sessionPolicy{}, true, &logs)
			require.Error(t, a.WriteConsolidatedMemoryActivity(context.Background(), activities.WriteMemoryInput{
				Entries: gateEntries(), Retention: retention,
			}))
			assert.Zero(t, counter.total)
		})
	}
	t.Run("session-core fails on the first entry", func(t *testing.T) {
		counter := swapGateCounter(t)
		var logs bytes.Buffer
		a := gatedActivities(t, &recordingLetta{}, nil, true, &logs)
		require.Error(t, a.WriteConsolidatedMemoryActivity(context.Background(), activities.WriteMemoryInput{
			Entries: gateEntries(), Retention: activities.RetentionDurable,
		}))
		assert.Zero(t, counter.total)
	})
}
