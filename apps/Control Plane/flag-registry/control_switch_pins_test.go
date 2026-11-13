package flagregistry

import (
	"slices"
	"strings"
	"testing"
)

const (
	cpUserCoreMain     = "apps/Control Plane/user-core/cmd/server/main.go"
	cpUserCoreHandlers = "apps/Control Plane/user-core/internal/http/handlers.go"
	cpUserCoreAudit    = "apps/Control Plane/user-core/internal/spaces/decision_audit.go"
	cpAuditCoreMain    = "apps/Control Plane/audit-core/cmd/server/main.go"
)

// cpSwitchPin is one Control Plane switch that the registry describes. Each stays a dark,
// default-off, Control Plane switch whose purpose ends with its strict sentence. A compose
// switch is declared once for its service, plus once for each service in alsoDeclaredBy, all
// with the same default. A code switch declares no compose entry.
type cpSwitchPin struct {
	name           string
	source         Source
	service        string   // the compose service that declares it; empty for a code switch
	alsoDeclaredBy []string // other compose services that set the same variable
	codePath       string
	strict         string // the literal word the reader accepts
	def            string
}

var cpSwitchPins = []cpSwitchPin{
	{name: "AUDIT_ALLOW_NATS_TOKEN_FALLBACK", source: SourceCode, codePath: cpAuditCoreMain, strict: "1", def: "0"},
	{name: "AUDIT_CHAIN_PRUNE_DRYRUN", source: SourceCompose, service: "audit-core", codePath: cpAuditCoreMain, strict: "1", def: "0"},
	{name: "CONTROL_OWNER_ASSERTED_GRANTS", source: SourceCompose, service: "user-core", codePath: cpUserCoreMain, strict: "true", def: "false"},
	{name: "CONTROL_SPACE_SHARED_IMPORT", source: SourceCompose, service: "user-core", codePath: cpUserCoreMain, strict: "true", def: "false"},
	{name: "CONTROL_SPACE_SUBJECT_GRANTS", source: SourceCompose, service: "user-core", codePath: cpUserCoreMain, strict: "true", def: "false"},
	{name: "PERSONAL_SETUP_VISIBLE", source: SourceCompose, service: "user-core", codePath: cpUserCoreHandlers, strict: "true", def: "false"},
	{name: "SCHEDULE_CONTRACT_ENABLED", source: SourceCompose, service: "user-core", alsoDeclaredBy: []string{"capability-core", "gateway"}, codePath: cpUserCoreMain, strict: "true", def: "false"},
	{name: "SPACE_AUDIT_DECISION_EVENTS", source: SourceCompose, service: "user-core", codePath: cpUserCoreAudit, strict: "true", def: "false"},
	{name: "SPACE_DM_REGISTRATION", source: SourceCompose, service: "user-core", codePath: cpUserCoreMain, strict: "true", def: "false"},
	{name: "SPACE_HISTORY_WINDOW_CLAIM", source: SourceCompose, service: "user-core", codePath: cpUserCoreMain, strict: "true", def: "false"},
	{name: "SPACE_MEMBERSHIP_EVENTS_SWEEPER", source: SourceCompose, service: "user-core", codePath: cpUserCoreMain, strict: "true", def: "false"},
	{name: "SPACE_POLICY_ADMIN_WRITES", source: SourceCompose, service: "user-core", codePath: cpUserCoreMain, strict: "true", def: "false"},
	{name: "SPACE_SANDBOX_EGRESS_TIERS", source: SourceCompose, service: "user-core", codePath: cpUserCoreMain, strict: "true", def: "false"},
	{name: "TONE_PRESETS_VISIBLE", source: SourceCompose, service: "user-core", codePath: cpUserCoreHandlers, strict: "true", def: "false"},
}

// TestControlPlaneSwitchesAreRegisteredAsPinned pins each switch of this batch: the owner,
// the dark stage, the default, the reader, the strict literal and the compose declarations. A
// change to any of them changes what a deployment does, so it must fail here first.
func TestControlPlaneSwitchesAreRegisteredAsPinned(t *testing.T) {
	reg := loadShipped(t)
	for _, p := range cpSwitchPins {
		f, ok := reg.Flag(p.name)
		if !ok {
			t.Errorf("%s has no registry entry", p.name)
			continue
		}
		if f.Kind != KindSwitch || f.Owner != PlaneControl || f.EffectiveStage() != StageDark {
			t.Errorf("%s: kind %q owner %q stage %q, want switch/control-plane/dark", p.name, f.Kind, f.Owner, f.EffectiveStage())
		}
		if f.Default != p.def {
			t.Errorf("%s: default %q, want %q", p.name, f.Default, p.def)
		}
		if f.Source != p.source {
			t.Errorf("%s: source %q, want %q", p.name, f.Source, p.source)
		}
		if f.CodePath != p.codePath {
			t.Errorf("%s: codePath %q, want %q", p.name, f.CodePath, p.codePath)
		}
		if f.OrgOverridable || f.KillSwitch || f.DevOnly {
			t.Errorf("%s: must be neither orgOverridable, a kill switch nor devOnly", p.name)
		}
		if want := "Only the literal " + p.strict + " turns it on."; !strings.HasSuffix(f.Purpose, want) {
			t.Errorf("%s: purpose must end with %q, got %q", p.name, want, f.Purpose)
		}
		if p.source == SourceCompose {
			var got []string
			for _, d := range f.Declarations {
				if d.Env != p.name || d.Default != p.def {
					t.Errorf("%s: declaration %+v, want env %s with default %q", p.name, d, p.name, p.def)
				}
				got = append(got, d.Service)
			}
			want := append([]string{p.service}, p.alsoDeclaredBy...)
			slices.Sort(got)
			slices.Sort(want)
			if !slices.Equal(got, want) {
				t.Errorf("%s: declared by services %v, want %v", p.name, got, want)
			}
		} else if len(f.Declarations) != 0 {
			t.Errorf("%s: a code switch declares no compose entry, got %+v", p.name, f.Declarations)
		}
	}
}

// TestAuditTokenFallbackSwitchSaysItRelaxesSecurity keeps the audit-core token fallback
// labelled as security-relaxing, so nobody reads it as an ordinary knob.
func TestAuditTokenFallbackSwitchSaysItRelaxesSecurity(t *testing.T) {
	f, ok := loadShipped(t).Flag("AUDIT_ALLOW_NATS_TOKEN_FALLBACK")
	if !ok {
		t.Fatal("AUDIT_ALLOW_NATS_TOKEN_FALLBACK is not registered")
	}
	if !strings.Contains(f.Purpose, "Security-relaxing") {
		t.Errorf("purpose must say the switch relaxes security: %q", f.Purpose)
	}
}

// TestScheduleContractSwitchNamesItsBlocker keeps SCHEDULE_CONTRACT_ENABLED off in the
// registry's words until its blocker is built: the purpose must name depth-register D0-010.
func TestScheduleContractSwitchNamesItsBlocker(t *testing.T) {
	f, ok := loadShipped(t).Flag("SCHEDULE_CONTRACT_ENABLED")
	if !ok {
		t.Fatal("SCHEDULE_CONTRACT_ENABLED is not registered")
	}
	if !strings.Contains(f.Purpose, "D0-010") {
		t.Errorf("purpose must name depth-register D0-010, its blocker: %q", f.Purpose)
	}
}
