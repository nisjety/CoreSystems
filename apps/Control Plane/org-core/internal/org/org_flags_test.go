package org

import (
	"context"
	"errors"
	"testing"
	"time"
)

// W5-079-b: a write is refused before storage when the name is outside the closed list
// or the value is not what the flag's kind allows. A zero Service has no repository, so
// reaching storage would panic: every case here must be refused first.
func TestSetOrgFlagRefusesBeforeStorage(t *testing.T) {
	service := &Service{}
	cases := []struct {
		name, value string
		want        error
	}{
		{"VITE_NOT_A_FLAG", "true", ErrUnknownOrgFlag},
		{"vite_chat_reader_polish", "true", ErrUnknownOrgFlag},
		{"WEB_POLICY_CLAIM_ENABLED", "true", ErrUnknownOrgFlag},
		{"VITE_CHAT_READER_POLISH", "TRUE", ErrInvalidOrgFlagValue},
		{"VITE_CHAT_READER_POLISH", "True", ErrInvalidOrgFlagValue},
		{"VITE_CHAT_READER_POLISH", "1", ErrInvalidOrgFlagValue},
		{"VITE_CHAT_READER_POLISH", "yes", ErrInvalidOrgFlagValue},
		{"VITE_CHAT_READER_POLISH", "", ErrInvalidOrgFlagValue},
		{"VITE_AGENT_RESULT_FEEDBACK", "on", ErrInvalidOrgFlagValue},
	}
	for _, tc := range cases {
		result, err := service.SetOrgFlag(context.Background(), "org-1", tc.name, tc.value, "u-owner", "owner")
		if result != nil || !errors.Is(err, tc.want) {
			t.Fatalf("SetOrgFlag(%q, %q) = %v, %v; want %v", tc.name, tc.value, result, err, tc.want)
		}
		var refusal *OrgFlagError
		if !errors.As(err, &refusal) || refusal.Name != tc.name {
			t.Fatalf("SetOrgFlag(%q, %q): refusal %v does not name the flag", tc.name, tc.value, err)
		}
	}
}

// A switch value is stored as canonical lower-case text: surrounding space is dropped,
// nothing else is rewritten.
func TestOrgFlagSwitchNormalisesToCanonicalText(t *testing.T) {
	def, ok := orgFlagDef("VITE_CHAT_CHANGED_FILES")
	if !ok || def.Kind != OrgFlagKindSwitch {
		t.Fatalf("VITE_CHAT_CHANGED_FILES is not a switch in the closed list")
	}
	for raw, want := range map[string]string{"true": "true", "false": "false", " true ": "true", "\tfalse\n": "false"} {
		got, ok := def.normalise(raw)
		if !ok || got != want {
			t.Fatalf("normalise(%q) = %q, %v; want %q", raw, got, ok, want)
		}
	}
	choice := OrgFlagDef{Name: "X", Kind: OrgFlagKindChoice, Values: []string{"a", "b"}, Default: "a"}
	if _, ok := choice.normalise("A"); ok {
		t.Fatal("a choice accepted a value outside its closed list")
	}
	if got, ok := choice.normalise(" b "); !ok || got != "b" {
		t.Fatalf("choice normalise(\" b \") = %q, %v", got, ok)
	}
}

// The closed list holds presentation switches only, each defaulting off, sorted by name.
func TestOrgOverridableFlagsArePresentationSwitchesDefaultOff(t *testing.T) {
	if len(OrgOverridableFlags) == 0 {
		t.Fatal("OrgOverridableFlags is empty")
	}
	for i, def := range OrgOverridableFlags {
		if def.Kind == OrgFlagKindSwitch && def.Default != "false" {
			t.Errorf("%s: a switch must default to \"false\", got %q", def.Name, def.Default)
		}
		if _, ok := def.normalise(def.Default); !ok {
			t.Errorf("%s: default %q is not a valid value of its own kind", def.Name, def.Default)
		}
		if i > 0 && OrgOverridableFlags[i-1].Name >= def.Name {
			t.Errorf("OrgOverridableFlags is not sorted by name at %s", def.Name)
		}
	}
}

// The audit row of a flag write is content-free: the closed name and value, the revision
// and the actor, under the Control org-core subject, with a stable id per write.
func TestOrgFlagAuditPayloadIsContentFree(t *testing.T) {
	change := orgFlagAuditChange{OrgID: "org-1", ActorID: "u-owner", ActorRole: "owner", Name: "VITE_CHAT_READER_POLISH", Value: "true", Revision: 4}
	at := time.Date(2026, 11, 12, 10, 0, 0, 0, time.UTC)
	id, payload := orgFlagAuditPayload(change, at)
	again, _ := orgFlagAuditPayload(change, at.Add(time.Hour))
	if id != again {
		t.Fatalf("the event id of one write must not depend on time: %q vs %q", id, again)
	}
	next := change
	next.Revision = 5
	if other, _ := orgFlagAuditPayload(next, at); other == id {
		t.Fatal("two revisions share one event id")
	}
	if payload["event"] != OrgFlagAuditEvent || payload["plane"] != "control" || payload["producer"] != "org-core" {
		t.Fatalf("payload names the wrong event: %v", payload)
	}
	if OrgFlagAuditSubject != "verevon.audit.v2.control.org-core.org_flag_changed" {
		t.Fatalf("subject %q", OrgFlagAuditSubject)
	}
	details, _ := payload["details"].(map[string]any)
	if len(details) != 3 || details["name"] != change.Name || details["value"] != change.Value || details["revision"] != change.Revision {
		t.Fatalf("details must be exactly name, value, revision: %v", details)
	}
}
