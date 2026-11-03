package spaces

import (
	"errors"
	"testing"
)

// WS12-S1: Registration.Validate for the DM kinds, Kind.AgentScope and the
// catalogue row, all without a database.

func dmRegistration(t *testing.T, kind Kind, owner string, participants ...string) Registration {
	t.Helper()
	hash, err := ParticipantSetHash(participants...)
	if err != nil {
		t.Fatalf("hash: %v", err)
	}
	return Registration{
		SpaceRef: "dm-space-1", OrgID: "org-1", OwnerPrincipalID: owner, Kind: kind,
		Lifecycle: LifecycleActive, LifecycleRevision: 1, Participants: participants, ParticipantHash: hash,
	}
}

func TestRegistrationValidateAcceptsDMAndGroupDM(t *testing.T) {
	if err := dmRegistration(t, KindDM, "u1", "u2", "u1").Validate(); err != nil {
		t.Fatalf("dm refused: %v", err)
	}
	if err := dmRegistration(t, KindGroupDM, "u2", "u1", "u2", "u3").Validate(); err != nil {
		t.Fatalf("group_dm refused: %v", err)
	}
	nine := []string{"p1", "p2", "p3", "p4", "p5", "p6", "p7", "p8", "p9"}
	if err := dmRegistration(t, KindGroupDM, "p5", nine...).Validate(); err != nil {
		t.Fatalf("nine-person group_dm refused: %v", err)
	}
}

func TestRegistrationValidateRefusesBadDMRosters(t *testing.T) {
	reasonOf := func(err error) string {
		var invalid *DMParticipantInvalidError
		if errors.As(err, &invalid) {
			return invalid.Reason
		}
		return ""
	}
	// Kind and count disagree.
	if err := dmRegistration(t, KindGroupDM, "u1", "u1", "u2").Validate(); reasonOf(err) != DMReasonCount {
		t.Errorf("group_dm with two people: err = %v, want reason count", err)
	}
	if err := dmRegistration(t, KindDM, "u1", "u1", "u2", "u3").Validate(); reasonOf(err) != DMReasonCount {
		t.Errorf("dm with three people: err = %v, want reason count", err)
	}
	// The owner must be one of the participants.
	if err := dmRegistration(t, KindDM, "u9", "u1", "u2").Validate(); reasonOf(err) != DMReasonOwnerMissing {
		t.Errorf("owner outside the roster: err = %v, want reason owner_missing", err)
	}
	// A hash Application did not compute from this roster.
	forged := dmRegistration(t, KindDM, "u1", "u1", "u2")
	forged.ParticipantHash = "sha256:" + "0000000000000000000000000000000000000000000000000000000000000000"
	if err := forged.Validate(); err == nil {
		t.Error("a registration with a mismatched participant hash was accepted")
	}
	missingHash := dmRegistration(t, KindDM, "u1", "u1", "u2")
	missingHash.ParticipantHash = ""
	if err := missingHash.Validate(); err == nil {
		t.Error("a DM without a participant hash was accepted")
	}
	// Duplicates, empties and a missing roster.
	dup := dmRegistration(t, KindDM, "u1", "u1", "u2")
	dup.Participants = []string{"u1", "u1"}
	if err := dup.Validate(); reasonOf(err) != DMReasonDuplicate {
		t.Errorf("duplicate participants: err = %v", err)
	}
	none := dmRegistration(t, KindDM, "u1", "u1", "u2")
	none.Participants = nil
	if err := none.Validate(); reasonOf(err) != DMReasonCount {
		t.Errorf("no participants: err = %v", err)
	}
}

func TestRegistrationValidateRefusesParticipantsOnNonDMKinds(t *testing.T) {
	for _, kind := range []Kind{KindPersonal, KindRoom, KindProject, KindCase} {
		base := Registration{
			SpaceRef: "s1", OrgID: "org-1", OwnerPrincipalID: "u1", Kind: kind,
			Lifecycle: LifecycleActive, LifecycleRevision: 1,
		}
		if err := base.Validate(); err != nil {
			t.Errorf("%s without participants refused: %v", kind, err)
		}
		withRoster := base
		withRoster.Participants = []string{"u1", "u2"}
		if err := withRoster.Validate(); err == nil {
			t.Errorf("%s with participants was accepted", kind)
		}
		withHash := base
		withHash.ParticipantHash = "sha256:abc"
		if err := withHash.Validate(); err == nil {
			t.Errorf("%s with a participant hash was accepted", kind)
		}
	}
}

func TestKindAgentScopeAndIsDM(t *testing.T) {
	for kind, want := range map[Kind]string{
		KindPersonal: AgentScopePersonal, KindRoom: AgentScopeRoom, KindProject: AgentScopeRoom, KindCase: AgentScopeRoom,
		KindDM: AgentScopeNone, KindGroupDM: AgentScopeNone, Kind("mystery"): AgentScopeNone, Kind(""): AgentScopeNone,
	} {
		if got := kind.AgentScope(); got != want {
			t.Errorf("%q.AgentScope() = %q, want %q", kind, got, want)
		}
	}
	for kind, want := range map[Kind]bool{KindDM: true, KindGroupDM: true, KindRoom: false, KindPersonal: false, Kind("dm "): false} {
		if kind.IsDM() != want {
			t.Errorf("%q.IsDM() = %v, want %v", kind, kind.IsDM(), want)
		}
	}
}

func TestDMCreateIsACatalogueEntitlementThatFailsClosedForDMs(t *testing.T) {
	spec, ok := EntitlementSpecByKey("dm_create")
	if !ok || spec.Column != "dm_create_entitled" || spec.ZDR != ZDRInert {
		t.Fatalf("dm_create row = %+v ok=%v", spec, ok)
	}
	if len(spec.Decisions) != 1 || spec.Decisions[0] != "space.dm.create" {
		t.Fatalf("decisions = %v", spec.Decisions)
	}
	// The three bits a DM needs are named with the catalogue's column spelling.
	for _, key := range dmEntitlementKeys {
		found := false
		for _, entry := range EntitlementCatalog() {
			if entry.Column == key {
				found = true
			}
		}
		if !found {
			t.Errorf("dm entitlement %q is not a catalogue column", key)
		}
	}
}
