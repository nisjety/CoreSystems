package preferences

import "testing"

func TestChannelEnabledOnlyTheExplicitRowDecides(t *testing.T) {
	rows := []Preference{
		{EventType: "modelplane.needs_approval", Channel: "email", Enabled: false},
	}
	// An email opt-out leaves in-app at its fallback: one channel off must not suppress another.
	if !channelEnabled(rows, "modelplane.needs_approval", "in_app", true) {
		t.Fatal("in_app = false, want the fallback: an email opt-out must not reach in-app")
	}
	if channelEnabled(rows, "modelplane.needs_approval", "email", true) {
		t.Fatal("email = true, want the explicit opt-out to hold")
	}
}

func TestChannelEnabledHonoursAnExplicitInAppOptOut(t *testing.T) {
	rows := []Preference{
		{EventType: "modelplane.needs_approval", Channel: "in_app", Enabled: false},
		{EventType: "modelplane.needs_approval", Channel: "email", Enabled: true},
	}
	if channelEnabled(rows, "modelplane.needs_approval", "in_app", true) {
		t.Fatal("in_app = true, want the explicit in-app opt-out to hold")
	}
}

func TestChannelEnabledIgnoresRowsOfOtherEventTypes(t *testing.T) {
	rows := []Preference{
		{EventType: "modelplane.run_completed", Channel: "in_app", Enabled: false},
	}
	if !channelEnabled(rows, "modelplane.needs_approval", "in_app", true) {
		t.Fatal("in_app = false, want a row for another event type to be ignored")
	}
}

func TestChannelEnabledFallsBackWithoutARow(t *testing.T) {
	if !channelEnabled(nil, "modelplane.needs_approval", "in_app", true) {
		t.Fatal("no row with fallback true = false, want true")
	}
	if channelEnabled(nil, "modelplane.needs_approval", "in_app", false) {
		t.Fatal("no row with fallback false = true, want false")
	}
}
