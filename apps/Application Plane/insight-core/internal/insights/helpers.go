package insights

import (
	"crypto/sha1"
	"encoding/hex"
	"errors"
	"fmt"
	"strings"
)

func IsInvalidInput(err error) bool {
	return errors.Is(err, ErrInvalidInput)
}

func normalizeSurface(value string) string {
	return normalizeToken(value)
}

func normalizeToken(value string) string {
	value = strings.TrimSpace(strings.ToLower(value))
	value = strings.ReplaceAll(value, "-", "_")
	value = strings.ReplaceAll(value, " ", "_")
	return value
}

func normalizeSurfaceList(values []string) []string {
	if len(values) == 0 {
		return SupportedSurfaces()
	}
	seen := map[string]bool{}
	surfaces := []string{}
	for _, value := range values {
		for _, part := range strings.Split(value, ",") {
			surface := normalizeSurface(part)
			if surface == "" || seen[surface] {
				continue
			}
			seen[surface] = true
			surfaces = append(surfaces, surface)
		}
	}
	if len(surfaces) == 0 {
		return SupportedSurfaces()
	}
	return surfaces
}

func SupportedSurfaces() []string {
	return []string{SurfaceSocial, SurfaceInbox, SurfaceAgents, SurfaceCampaigns, SurfaceExternalAnalytics}
}

func isSupportedSurface(surface string) bool {
	for _, supported := range SupportedSurfaces() {
		if surface == supported {
			return true
		}
	}
	return false
}

func surfaceSet(surfaces []string) map[string]struct{} {
	result := make(map[string]struct{}, len(surfaces))
	for _, surface := range surfaces {
		result[surface] = struct{}{}
	}
	return result
}

func stableEventID(event MetricEvent) string {
	hash := sha1.Sum([]byte(fmt.Sprintf("%s|%s|%s|%s|%f|%d", event.OrgID, event.Surface, event.Metric, event.Source, event.Value, event.OccurredAt.UnixNano())))
	return "ins_evt_" + hex.EncodeToString(hash[:10])
}

func fallback(value, defaultValue string) string {
	value = strings.TrimSpace(value)
	if value == "" {
		return defaultValue
	}
	return value
}

func copyMetricEvent(event MetricEvent) MetricEvent {
	return MetricEvent{
		ID:            event.ID,
		OrgID:         event.OrgID,
		Surface:       event.Surface,
		Metric:        event.Metric,
		Value:         event.Value,
		Unit:          event.Unit,
		Source:        event.Source,
		ConnectorType: event.ConnectorType,
		Dimensions:    copyMap(event.Dimensions),
		OccurredAt:    event.OccurredAt,
	}
}

func copyMap(input map[string]any) map[string]any {
	if len(input) == 0 {
		return map[string]any{}
	}
	output := make(map[string]any, len(input))
	for key, value := range input {
		output[key] = value
	}
	return output
}

func copyConnectorSlots(input []ConnectorSlot) []ConnectorSlot {
	output := make([]ConnectorSlot, 0, len(input))
	for _, slot := range input {
		output = append(output, ConnectorSlot{
			Type:               slot.Type,
			DisplayName:        slot.DisplayName,
			Surface:            slot.Surface,
			Status:             slot.Status,
			Authorization:      slot.Authorization,
			TokenLeaseAudience: slot.TokenLeaseAudience,
			RequiredEnv:        copyStrings(slot.RequiredEnv),
			ReferenceURLs:      copyStrings(slot.ReferenceURLs),
			Contracts:          copyContracts(slot.Contracts),
		})
	}
	return output
}

func copyContracts(input []ConnectorContract) []ConnectorContract {
	output := make([]ConnectorContract, 0, len(input))
	for _, contract := range input {
		output = append(output, ConnectorContract{
			Name:              contract.Name,
			EndpointTemplate:  contract.EndpointTemplate,
			Method:            contract.Method,
			RequestShape:      contract.RequestShape,
			ResponseShape:     contract.ResponseShape,
			RequiredScopes:    copyStrings(contract.RequiredScopes),
			DimensionExamples: copyStrings(contract.DimensionExamples),
			MetricExamples:    copyStrings(contract.MetricExamples),
		})
	}
	return output
}

func copyStrings(input []string) []string {
	if len(input) == 0 {
		return []string{}
	}
	output := make([]string, len(input))
	copy(output, input)
	return output
}
