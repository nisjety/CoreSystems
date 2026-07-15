package billing

import (
	"bytes"
	"encoding/json"
	"fmt"
	"math"
	"regexp"
	"strings"
)

const (
	MaxUsageEventIDLength = 128
	MaxUsageOrgIDLength   = 128
	MaxUsageMetricLength  = 128
	MaxUsageSourceLength  = 128
	MaxUsageMetadataBytes = 16 * 1024
	MaxUsageMetadataDepth = 8
	MaxUsageMetadataNodes = 512
	MaxUsageQuantity      = 1_000_000_000_000
)

var usageEventIDPattern = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._:-]*$`)
var usageDimensionPattern = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._:/-]*$`)

func ValidateUsageEventID(eventID string) error {
	if eventID == "" || eventID != strings.TrimSpace(eventID) {
		return fmt.Errorf("event_id is required and must not contain surrounding whitespace")
	}
	if len(eventID) > MaxUsageEventIDLength {
		return fmt.Errorf("event_id must be at most %d bytes", MaxUsageEventIDLength)
	}
	if !usageEventIDPattern.MatchString(eventID) {
		return fmt.Errorf("event_id contains unsupported characters")
	}
	return nil
}

func ValidateUsageEvent(usage UsageEvent) error {
	if err := ValidateUsageEventID(usage.EventID); err != nil {
		return err
	}
	if err := validateUsageDimension("org_id", usage.OrgID, MaxUsageOrgIDLength, false); err != nil {
		return err
	}
	if err := validateUsageDimension("metric", usage.Metric, MaxUsageMetricLength, false); err != nil {
		return err
	}
	if usage.Source != "" {
		if err := validateUsageDimension("source", usage.Source, MaxUsageSourceLength, true); err != nil {
			return err
		}
	}
	if usage.OccurredAt.IsZero() {
		return fmt.Errorf("occurred_at is required")
	}
	if math.IsNaN(usage.Quantity) || math.IsInf(usage.Quantity, 0) || usage.Quantity <= 0 || usage.Quantity > MaxUsageQuantity {
		return fmt.Errorf("quantity must be a positive finite number no greater than %g", float64(MaxUsageQuantity))
	}
	if err := validateUsageMetadata(usage.Metadata); err != nil {
		return err
	}
	return nil
}

func validateUsageDimension(name, value string, maxBytes int, allowSlash bool) error {
	if value == "" || value != strings.TrimSpace(value) {
		return fmt.Errorf("%s is required and must not contain surrounding whitespace", name)
	}
	if len(value) > maxBytes {
		return fmt.Errorf("%s must be at most %d bytes", name, maxBytes)
	}
	if !usageDimensionPattern.MatchString(value) || (!allowSlash && strings.Contains(value, "/")) {
		return fmt.Errorf("%s contains unsupported characters", name)
	}
	return nil
}

func validateUsageMetadata(metadata map[string]interface{}) error {
	if metadata == nil {
		return nil
	}
	encoded, err := json.Marshal(metadata)
	if err != nil {
		return fmt.Errorf("metadata must be valid JSON: %w", err)
	}
	if len(encoded) > MaxUsageMetadataBytes {
		return fmt.Errorf("metadata must be at most %d bytes", MaxUsageMetadataBytes)
	}
	decoder := json.NewDecoder(bytes.NewReader(encoded))
	decoder.UseNumber()
	var canonical interface{}
	if err := decoder.Decode(&canonical); err != nil {
		return fmt.Errorf("metadata must be valid JSON: %w", err)
	}
	nodes := 0
	if err := validateUsageMetadataNode(canonical, 1, &nodes); err != nil {
		return err
	}
	return nil
}

func validateUsageMetadataNode(value interface{}, depth int, nodes *int) error {
	if depth > MaxUsageMetadataDepth {
		return fmt.Errorf("metadata nesting exceeds %d levels", MaxUsageMetadataDepth)
	}
	(*nodes)++
	if *nodes > MaxUsageMetadataNodes {
		return fmt.Errorf("metadata exceeds %d values", MaxUsageMetadataNodes)
	}
	switch typed := value.(type) {
	case map[string]interface{}:
		for key, child := range typed {
			if key == "" || len(key) > 128 {
				return fmt.Errorf("metadata keys must be 1-128 bytes")
			}
			if err := validateUsageMetadataNode(child, depth+1, nodes); err != nil {
				return err
			}
		}
	case []interface{}:
		for _, child := range typed {
			if err := validateUsageMetadataNode(child, depth+1, nodes); err != nil {
				return err
			}
		}
	}
	return nil
}
