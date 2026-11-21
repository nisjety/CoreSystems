package main

import (
	"testing"

	"github.com/triodelab/model-plane/services/capability-core/internal/learning"
)

// TestLearningPIIScreenIsInstalledOnlyByTheLiteralTrue pins the switch: only the literal true installs the screen, and
// any other value leaves learning.Screen as it was.
func TestLearningPIIScreenIsInstalledOnlyByTheLiteralTrue(t *testing.T) {
	original := learning.Screen
	t.Cleanup(func() { learning.Screen = original })
	withAddress := learning.SkillCandidate{Name: "n", Content: "mail kari.nordmann@example.com", Confidence: 1}

	for _, raw := range []string{"", "false", "0", "1", "yes", "on", "TRUE", " true "} {
		learning.Screen = original
		lookup := func(key string) string {
			if key == learningPIIScreenEnv {
				return raw
			}
			return ""
		}
		if installLearningPIIScreen(lookup) {
			t.Fatalf("%s=%q installed the screen; only the literal true may", learningPIIScreenEnv, raw)
		}
		if !learning.Screen(withAddress) {
			t.Fatalf("%s=%q changed learning.Screen; an off switch must assign nothing", learningPIIScreenEnv, raw)
		}
	}

	learning.Screen = original
	on := func(key string) string {
		if key == learningPIIScreenEnv {
			return "true"
		}
		return ""
	}
	if !installLearningPIIScreen(on) {
		t.Fatalf("%s=true must install the screen", learningPIIScreenEnv)
	}
	if learning.Screen(withAddress) {
		t.Fatalf("with %s=true the screen must drop a candidate that carries an e-mail address", learningPIIScreenEnv)
	}
}
