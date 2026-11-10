package eventtrigger

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"slices"
)

// The trigger dry run (EP-7, F-t3-047, slice W4-F T6) asks the wake gate one question about one stored webhook event: would
// a fire of this trigger wake, and why. It takes the gate's own decision from Evaluate, reads the event through the source the
// gate reads, and holds the payload in memory for that one evaluation. Its result carries no payload, no field value and no
// part of the event: a verdict, one fixed reason token and the names of the rules the event satisfied. It records no fire,
// starts nothing and asks Control nothing.

// DryRunNoWakeFilter is the reason a dry run gives for a trigger with no wake filter. The gate does not run for such a
// trigger, so the dry run reads no payload and the trigger wakes.
const DryRunNoWakeFilter = "no_wake_filter"

// DryRunFilterOutsideSchema is the reason a dry run gives for a stored filter the closed schema does not allow. The gate
// closes for such a filter, and the dry run reads no payload, because the gate parses the filter before it reads.
const DryRunFilterOutsideSchema = "filter_outside_schema"

// dryRuleEventTypes names the event_types rule in the matched list of a dry run.
const dryRuleEventTypes = "event_types"

// dryRuleProviders names the providers rule in the matched list of a dry run.
const dryRuleProviders = "providers"

// dryRuleSubjectContains names the subject_contains rule in the matched list of a dry run.
const dryRuleSubjectContains = "subject_contains"

// DryRunResult is what a dry run of the wake gate reports for one webhook event. WouldWake is the gate's verdict and Reason
// its one fixed token, or one of the dry run reasons above. Matched names every rule of the stored filter the event satisfies,
// in filter order. A rule is named by its category, and a field_equals rule by its position in the filter, so no name carries
// a path's value or a payload value. Matched is empty for a trigger with no filter and for a filter outside the schema.
type DryRunResult struct {
	WouldWake bool
	Reason    string
	Matched   []string
}

// DryRunWakeGate evaluates a trigger's stored wake filter over one webhook event of orgID and reports the result. A trigger
// with no filter, and a filter the closed schema does not allow, read no payload. Any other filter reads the event through
// source, as the gate does, and an error from that read means the event could not be read now. The caller reports that
// error and nothing else; no part of the event is ever returned or logged here.
func DryRunWakeGate(ctx context.Context, source WebhookEventSource, orgID, webhookEventID string, wakeFilter json.RawMessage) (DryRunResult, error) {
	if len(wakeFilter) == 0 {
		return DryRunResult{WouldWake: true, Reason: DryRunNoWakeFilter, Matched: []string{}}, nil
	}
	filter, err := ParseWakeFilter(wakeFilter)
	if err != nil {
		return DryRunResult{WouldWake: false, Reason: DryRunFilterOutsideSchema, Matched: []string{}}, nil
	}
	if source == nil {
		return DryRunResult{}, errors.New("wake gate has no webhook event source")
	}
	event, err := source.FetchWebhookEvent(ctx, orgID, webhookEventID)
	if err != nil {
		return DryRunResult{}, err
	}
	payload := event.view()
	wouldWake, reason := Evaluate(filter, payload)
	return DryRunResult{WouldWake: wouldWake, Reason: reason, Matched: matchedRules(filter, payload)}, nil
}

// matchedRules names each rule of filter the payload satisfies, in filter order. Evaluate stops at the first rule that does
// not hold, which is why the gate closed; this does not stop, so a dry run can say which rules hold as well. It reads the
// same keys and makes the same comparisons as Evaluate.
func matchedRules(filter WakeFilter, payload map[string]any) []string {
	matched := []string{}
	if len(filter.EventTypes) > 0 {
		if eventType, _ := payload[wakeKeyEventType].(string); slices.Contains(filter.EventTypes, eventType) {
			matched = append(matched, dryRuleEventTypes)
		}
	}
	if len(filter.Providers) > 0 {
		if providerKey, _ := payload[wakeKeyProviderKey].(string); slices.Contains(filter.Providers, providerKey) {
			matched = append(matched, dryRuleProviders)
		}
	}
	for i, field := range filter.FieldEquals {
		if value, ok := lookupString(payload, field.Path); ok && value == field.Value {
			matched = append(matched, fmt.Sprintf("field_equals[%d]", i))
		}
	}
	if len(filter.SubjectContains) > 0 {
		if subject, ok := payload[wakeKeySubject].(string); ok && containsFold(subject, filter.SubjectContains) {
			matched = append(matched, dryRuleSubjectContains)
		}
	}
	return matched
}
