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
	return []string{
		SurfaceSocial,
		SurfaceInbox,
		SurfaceAgents,
		SurfaceChat,
		SurfaceKnowledge,
		SurfaceIngestion,
		SurfaceCampaigns,
		SurfaceExternalAnalytics,
	}
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
	hash := sha1.Sum([]byte(fmt.Sprintf("%s|%s|%s|%s|%s|%f|%d|%s", event.OrgID, event.ActorUserID, event.Surface, event.Metric, event.Source, event.Value, event.OccurredAt.UnixNano(), event.SourceTraceRef)))
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
		ID:             event.ID,
		OrgID:          event.OrgID,
		ActorUserID:    event.ActorUserID,
		Surface:        event.Surface,
		Metric:         event.Metric,
		Value:          event.Value,
		Unit:           event.Unit,
		Source:         event.Source,
		ConnectorType:  event.ConnectorType,
		Dimensions:     copyMap(event.Dimensions),
		TimeWindow:     copyTimeWindow(event.TimeWindow),
		ObservedAt:     event.ObservedAt,
		SourceTraceRef: event.SourceTraceRef,
		RetentionClass: event.RetentionClass,
		OccurredAt:     event.OccurredAt,
	}
}

func copyTimeWindow(input *TimeWindow) *TimeWindow {
	if input == nil {
		return nil
	}
	output := &TimeWindow{}
	if input.From != nil {
		from := *input.From
		output.From = &from
	}
	if input.To != nil {
		to := *input.To
		output.To = &to
	}
	return output
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
			Type:                     slot.Type,
			DisplayName:              slot.DisplayName,
			Surface:                  slot.Surface,
			Status:                   slot.Status,
			Authorization:            slot.Authorization,
			TokenLeaseAudience:       slot.TokenLeaseAudience,
			RequiredCapabilities:     copyStrings(slot.RequiredCapabilities),
			RequiredEnv:              copyStrings(slot.RequiredEnv),
			ReferenceURLs:            copyStrings(slot.ReferenceURLs),
			Contracts:                copyContracts(slot.Contracts),
			IntegrationSteps:         copyIntegrationSteps(slot.IntegrationSteps),
			AdvisoryInstructionPacks: copyAdvisoryInstructionPacks(slot.AdvisoryInstructionPacks),
			ModelGrounding: ModelGroundingContract{
				Contract:                  slot.ModelGrounding.Contract,
				RequiredFields:            copyStrings(slot.ModelGrounding.RequiredFields),
				ExcludesZeroDataRetention: slot.ModelGrounding.ExcludesZeroDataRetention,
			},
		})
	}
	return output
}

func copyIntegrationSteps(input []IntegrationStep) []IntegrationStep {
	output := make([]IntegrationStep, 0, len(input))
	for _, step := range input {
		output = append(output, IntegrationStep{
			ID:                   step.ID,
			Label:                step.Label,
			Purpose:              step.Purpose,
			EndpointTemplate:     step.EndpointTemplate,
			Method:               step.Method,
			FixedOperation:       step.FixedOperation,
			RequiredCapabilities: copyStrings(step.RequiredCapabilities),
		})
	}
	return output
}

func copyAdvisoryInstructionPacks(input []AdvisoryInstructionPack) []AdvisoryInstructionPack {
	output := make([]AdvisoryInstructionPack, 0, len(input))
	for _, pack := range input {
		output = append(output, AdvisoryInstructionPack{
			ID:                pack.ID,
			Mode:              pack.Mode,
			RequiresReadiness: pack.RequiresReadiness,
			SkillIDs:          copyStrings(pack.SkillIDs),
			Instructions:      copyStrings(pack.Instructions),
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
