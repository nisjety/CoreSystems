package billing

import (
	"context"
	"encoding/json"
	"math"
	"strings"
	"testing"
	"time"
)

func TestRecordUsageRejectsUnstableIdentityBeforeStorage(t *testing.T) {
	validTime := time.Date(2026, 7, 15, 10, 0, 0, 0, time.UTC)
	tests := []struct {
		name  string
		usage UsageEvent
	}{
		{
			name:  "missing event id",
			usage: UsageEvent{OrgID: "org-usage", Metric: "api_calls", Quantity: 1, OccurredAt: validTime},
		},
		{
			name:  "oversized event id",
			usage: UsageEvent{EventID: strings.Repeat("a", 129), OrgID: "org-usage", Metric: "api_calls", Quantity: 1, OccurredAt: validTime},
		},
		{
			name:  "invalid event id",
			usage: UsageEvent{EventID: "usage id", OrgID: "org-usage", Metric: "api_calls", Quantity: 1, OccurredAt: validTime},
		},
		{
			name:  "missing occurred at",
			usage: UsageEvent{EventID: "usage_01", OrgID: "org-usage", Metric: "api_calls", Quantity: 1},
		},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			service := &Service{}
			if err := service.RecordUsage(context.Background(), test.usage); err == nil {
				t.Fatal("invalid usage event was accepted")
			}
		})
	}
}

func TestRepositoryRecordUsageRejectsInvalidBoundaryBeforeStorage(t *testing.T) {
	repo := &Repository{}
	if recorded, err := repo.RecordUsage(context.Background(), UsageEvent{
		OrgID:      "org-usage",
		Metric:     "api_calls",
		Quantity:   1,
		OccurredAt: time.Date(2026, 7, 15, 10, 0, 0, 0, time.UTC),
	}); err == nil || recorded {
		t.Fatalf("invalid repository usage recorded=%t err=%v", recorded, err)
	}
}

func TestUsageRetryPayloadRejectsMissingStableFields(t *testing.T) {
	valid := map[string]interface{}{
		"event_id":    "usage_01",
		"org_id":      "org-usage",
		"metric":      "api_calls",
		"quantity":    float64(1),
		"occurred_at": "2026-07-15T10:00:00Z",
	}

	for _, field := range []string{"event_id", "occurred_at"} {
		payload := make(map[string]interface{}, len(valid))
		for key, value := range valid {
			payload[key] = value
		}
		delete(payload, field)
		if _, err := usageFromPayload(payload); err == nil {
			t.Fatalf("retry payload missing %s was accepted", field)
		}
	}

	invalidTimestamp := make(map[string]interface{}, len(valid))
	for key, value := range valid {
		invalidTimestamp[key] = value
	}
	invalidTimestamp["occurred_at"] = "not-a-timestamp"
	if _, err := usageFromPayload(invalidTimestamp); err == nil {
		t.Fatal("retry payload with invalid occurred_at was accepted")
	}
}

func TestValidateUsageEventBoundsDurablePayload(t *testing.T) {
	valid := UsageEvent{
		EventID:    "usage_01",
		OrgID:      "org-usage",
		Metric:     "api_calls",
		Quantity:   1,
		Source:     "model-plane",
		OccurredAt: time.Date(2026, 7, 15, 10, 0, 0, 0, time.UTC),
		Metadata:   map[string]interface{}{"run_id": "run-1"},
	}
	if err := ValidateUsageEvent(valid); err != nil {
		t.Fatalf("valid usage rejected: %v", err)
	}

	tests := []struct {
		name   string
		mutate func(*UsageEvent)
	}{
		{name: "zero quantity", mutate: func(event *UsageEvent) { event.Quantity = 0 }},
		{name: "negative quantity", mutate: func(event *UsageEvent) { event.Quantity = -1 }},
		{name: "nan quantity", mutate: func(event *UsageEvent) { event.Quantity = math.NaN() }},
		{name: "infinite quantity", mutate: func(event *UsageEvent) { event.Quantity = math.Inf(1) }},
		{name: "absurd quantity", mutate: func(event *UsageEvent) { event.Quantity = MaxUsageQuantity + 1 }},
		{name: "oversized org", mutate: func(event *UsageEvent) { event.OrgID = strings.Repeat("o", MaxUsageOrgIDLength+1) }},
		{name: "oversized metric", mutate: func(event *UsageEvent) { event.Metric = strings.Repeat("m", MaxUsageMetricLength+1) }},
		{name: "oversized source", mutate: func(event *UsageEvent) { event.Source = strings.Repeat("s", MaxUsageSourceLength+1) }},
		{name: "oversized metadata", mutate: func(event *UsageEvent) {
			event.Metadata = map[string]interface{}{"payload": strings.Repeat("x", MaxUsageMetadataBytes)}
		}},
		{name: "deep metadata", mutate: func(event *UsageEvent) {
			var value interface{} = "leaf"
			for depth := 0; depth < MaxUsageMetadataDepth; depth++ {
				value = map[string]interface{}{"child": value}
			}
			event.Metadata = map[string]interface{}{"root": value}
		}},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			candidate := valid
			test.mutate(&candidate)
			if err := ValidateUsageEvent(candidate); err == nil {
				t.Fatal("unbounded usage event was accepted")
			}
		})
	}
}

func TestUsageRetryPayloadCanonicalizesMetadataKeyOrder(t *testing.T) {
	base := UsageEvent{
		EventID:    "usage_01",
		OrgID:      "org-usage",
		Metric:     "api_calls",
		Quantity:   1,
		Source:     "model-plane",
		OccurredAt: time.Date(2026, 7, 15, 10, 0, 0, 0, time.UTC),
	}
	first := base
	first.Metadata = map[string]interface{}{"alpha": 1, "beta": "two"}
	second := base
	second.Metadata = map[string]interface{}{"beta": "two", "alpha": 1}
	firstJSON, err := json.Marshal(usageRetryPayload(first))
	if err != nil {
		t.Fatal(err)
	}
	secondJSON, err := json.Marshal(usageRetryPayload(second))
	if err != nil {
		t.Fatal(err)
	}
	if string(firstJSON) != string(secondJSON) {
		t.Fatalf("canonical usage payload differs by map insertion order:\n%s\n%s", firstJSON, secondJSON)
	}
}

func TestValidateUsageMetadataBoundsKeysNodesAndArrays(t *testing.T) {
	valid := UsageEvent{
		EventID:    "usage_01",
		OrgID:      "org-usage",
		Metric:     "api_calls",
		Quantity:   1,
		OccurredAt: time.Date(2026, 7, 15, 10, 0, 0, 0, time.UTC),
		Metadata: map[string]interface{}{
			"items": []interface{}{"one", float64(2), true, nil},
		},
	}
	if err := ValidateUsageEvent(valid); err != nil {
		t.Fatalf("bounded metadata array rejected: %v", err)
	}

	invalidKey := valid
	invalidKey.Metadata = map[string]interface{}{"": "value"}
	if err := ValidateUsageEvent(invalidKey); err == nil {
		t.Fatal("blank metadata key accepted")
	}

	tooMany := valid
	values := make([]interface{}, MaxUsageMetadataNodes+1)
	for index := range values {
		values[index] = index
	}
	tooMany.Metadata = map[string]interface{}{"items": values}
	if err := ValidateUsageEvent(tooMany); err == nil {
		t.Fatal("metadata node limit was not enforced")
	}
}

func TestClaimDueRetryJobsRejectsInvalidLimitBeforeStorage(t *testing.T) {
	repo := &Repository{}
	if _, err := repo.ClaimDueRetryJobs(context.Background(), 0); err == nil {
		t.Fatal("non-positive retry claim limit was accepted")
	}
}

func TestSaveAccountStateCASRejectsInvalidJSONBeforeStorage(t *testing.T) {
	repo := &Repository{}
	if err := repo.SaveAccountStateCAS(context.Background(), Account{
		OrgID:       "org-usage",
		QuotaLimits: map[string]float64{"invalid": math.NaN()},
	}); err == nil || !strings.Contains(err.Error(), "quota limits") {
		t.Fatalf("invalid quota JSON error=%v", err)
	}
	if err := repo.SaveAccountStateCAS(context.Background(), Account{
		OrgID:    "org-usage",
		Metadata: map[string]interface{}{"invalid": make(chan struct{})},
	}); err == nil || !strings.Contains(err.Error(), "metadata") {
		t.Fatalf("invalid metadata JSON error=%v", err)
	}
}
