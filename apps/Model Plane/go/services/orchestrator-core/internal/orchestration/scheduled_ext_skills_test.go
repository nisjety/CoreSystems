package orchestration

import (
	"encoding/json"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/workflows"
)

// scheduledRunRequestWithExtSkills is the scheduled-run request capability-core sends for a fire, with
// the ext_skills word it states (P2-ec-01-rel1). An empty word is sent as absent, as a switch-off fire does.
func scheduledRunRequestWithExtSkills(t *testing.T, templateJSON, templateDigest, word string) json.RawMessage {
	t.Helper()
	var fields map[string]any
	require.NoError(t, json.Unmarshal(scheduledRunBuildRequest(t, templateJSON, templateDigest), &fields))
	if word != "" {
		fields["ext_skills"] = word
	}
	raw, err := json.Marshal(fields)
	require.NoError(t, err)
	return raw
}

// TestScheduledRunInputCarriesTheExtSkillsWordOnlyAsStated pins P2-ec-01-rel1 at the orchestrator's
// input: a policy word passes into the workflow input unchanged, an absent word stays absent so the
// input is what it was, and any other word is refused before Temporal is ever reached.
func TestScheduledRunInputCarriesTheExtSkillsWordOnlyAsStated(t *testing.T) {
	tenancy := Tenancy{OrgID: "org-a", RunID: "task_scheduled_1", RetentionAttested: true}
	templateJSON, templateDigest := scheduledTemplateInput(t, map[string]any{"title": "Nightly queue review"})

	for _, word := range []string{"", "none", "approved_only", "any"} {
		built, err := buildScheduledRunInput(scheduledRunRequestWithExtSkills(t, templateJSON, templateDigest, word), tenancy)
		require.NoError(t, err, "word %q", word)
		input := built.(workflows.ScheduledRunInput)
		assert.Equal(t, word, input.ExtSkills, "word %q", word)
		if word == "" {
			encoded, err := json.Marshal(input)
			require.NoError(t, err)
			assert.NotContains(t, string(encoded), "ext_skills", "a fire the switch did not state a word for keeps its input")
		}
	}

	for _, word := range []string{"approved", "ANY", " any", "true"} {
		_, err := buildScheduledRunInput(scheduledRunRequestWithExtSkills(t, templateJSON, templateDigest, word), tenancy)
		assert.Error(t, err, "word %q is not an ext.skills policy word", word)
	}
}
