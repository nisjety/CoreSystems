package flagregistry

import (
	"os"
	"slices"
	"strings"
	"testing"
)

// modelComposePin is one Model Plane compose variable outside the naming convention.
// The compose ratchet scans only convention names, so nothing else notices if its entry
// goes missing. services lists the compose services that set it, in name order.
type modelComposePin struct {
	name     string
	kind     Kind
	def      string
	values   []string
	services []string
}

// modelComposePins are the Model Plane switches and choices that the ratchet does not
// scan but that a deployment sets. Each needs its own entry: owner model-plane, source
// compose, and the kind, default and values the compose file writes.
var modelComposePins = []modelComposePin{
	{name: "EXECUTION_CORE_SPACE_APPROVAL_GATE", kind: KindChoice, def: "enforce", values: []string{"off", "shadow", "enforce"}, services: []string{"execution-core"}},
	{name: "MODEL_ACTION_ADMISSION", kind: KindChoice, def: "", values: []string{"", "table"}, services: []string{"capability-core", "execution-core"}},
	{name: "ORG_AGENT_POLICY_API", kind: KindSwitch, def: "false", services: []string{"capability-core"}},
	{name: "WORKSPACE_TOOLS", kind: KindChoice, def: "off", values: []string{"off", "read", "all"}, services: []string{"execution-core"}},
}

// TestModelPlaneComposePinsStayRegistered keeps each pinned variable registered with the
// Model Plane as owner and compose as source. Its kind, default and values match the compose
// file, its declarations name every service that sets it, and it is not on the unregistered
// baseline. The ratchet then checks each declaration against the compose file itself.
func TestModelPlaneComposePinsStayRegistered(t *testing.T) {
	reg := loadShipped(t)
	for _, p := range modelComposePins {
		f, ok := reg.Flag(p.name)
		if !ok {
			t.Errorf("%s is set by the Model Plane compose but has no registry entry", p.name)
			continue
		}
		if f.Owner != PlaneModel {
			t.Errorf("%s: owner %q, want %q", p.name, f.Owner, PlaneModel)
		}
		if f.Source != SourceCompose {
			t.Errorf("%s: source %q, want %q", p.name, f.Source, SourceCompose)
		}
		if f.Kind != p.kind || f.Default != p.def {
			t.Errorf("%s: kind %q default %q, want kind %q default %q", p.name, f.Kind, f.Default, p.kind, p.def)
		}
		if p.kind == KindChoice && !slices.Equal(f.Values, p.values) {
			t.Errorf("%s: values %q, want %q", p.name, f.Values, p.values)
		}
		var services []string
		for _, d := range f.Declarations {
			if d.Env != p.name || d.Default != p.def {
				t.Errorf("%s: declaration %+v, want env %s with default %q", p.name, d, p.name, p.def)
			}
			services = append(services, d.Service)
		}
		slices.Sort(services)
		if !slices.Equal(services, p.services) {
			t.Errorf("%s: declared by services %v, want %v", p.name, services, p.services)
		}
		if slices.Contains(reg.Unregistered(), p.name) {
			t.Errorf("%s is listed as unregistered; its entry replaces that line", p.name)
		}
	}
}

// TestModelPlaneComposePinsReadOffWhenUnset pins the switch-off state a deployment gets when
// it leaves these variables unset: the workspace tools are off and the org agent policy API is
// off. The typed reads return the registry default for an unset variable, and that default is
// the compose default.
func TestModelPlaneComposePinsReadOffWhenUnset(t *testing.T) {
	reg := loadShipped(t)
	unset := func(string) (string, bool) { return "", false }
	if got, err := reg.Choice("WORKSPACE_TOOLS", unset); err != nil || got != "off" {
		t.Errorf("WORKSPACE_TOOLS unset reads %q (err %v), want off", got, err)
	}
	if on, err := reg.Switch("ORG_AGENT_POLICY_API", unset); err != nil || on {
		t.Errorf("ORG_AGENT_POLICY_API unset reads %v (err %v), want off", on, err)
	}
}

// strictPinNames are Model Plane switches that read only the exact literal true and default to 0 in the
// Model Plane compose. The pin test must find each one, so it cannot quietly stop covering them.
var strictPinNames = []string{
	"DREAMING_LLM_EXTRACTION",
	"MEMORY_GROUNDING_ENABLED",
	"SESSION_CORE_APPROVAL_MEMBER_DECIDE",
	"SPACE_ATTENTION_SUMMARY",
	"SPACE_HUMAN_POSTS_CORE",
	"SPACE_MENTION_RECORDS_CORE",
	"SPACE_MESSAGE_REVISE_CORE",
	"SPACE_MESSAGE_SEARCH_CORE",
	"SPACE_TIMELINE_CORE",
	"SPACE_WAKE_CORE",
}

// TestZeroDefaultLiteralSwitchesNameTheLiteralInCompose pins the sentence an operator reads. A switch that
// reads only the exact literal true treats 1, yes and on as off, so a compose line that defaults it to 0 must
// say, in the comment above it, which value turns it on. Without that, an operator who types 1 gets a switch
// that stays off with no word about why.
func TestZeroDefaultLiteralSwitchesNameTheLiteralInCompose(t *testing.T) {
	reg := loadShipped(t)
	data, err := os.ReadFile(repoFile(modelPlaneCompose))
	if err != nil {
		t.Fatalf("read %s: %v", modelPlaneCompose, err)
	}
	lines := strings.Split(strings.ReplaceAll(string(data), "\r\n", "\n"), "\n")
	found := make(map[string]bool)
	for i, line := range lines {
		m := composeEnv.FindStringSubmatch(line)
		if m == nil || m[3] != "0" {
			continue
		}
		f, ok := reg.Flag(m[2])
		if !ok || f.OnLiteral == "" {
			continue
		}
		found[m[2]] = true
		said := strings.ReplaceAll(commentAbove(lines, i)+" "+m[4], "`", "")
		if !strings.Contains(said, "literal "+f.OnLiteral) {
			t.Errorf("%s (%s line %d) defaults to 0 and reads only the literal %s, but its compose comment does not say so", m[2], modelPlaneCompose, i+1, f.OnLiteral)
		}
	}
	for _, name := range strictPinNames {
		if !found[name] {
			t.Errorf("%s is not a literal switch that defaults to 0 in the Model Plane compose; the pin no longer covers it", name)
		}
	}
}

// commentAbove joins the comment lines directly above line i, in file order.
func commentAbove(lines []string, i int) string {
	var block []string
	for j := i - 1; j >= 0 && strings.HasPrefix(strings.TrimSpace(lines[j]), "#"); j-- {
		block = append([]string{lines[j]}, block...)
	}
	return strings.Join(block, " ")
}
