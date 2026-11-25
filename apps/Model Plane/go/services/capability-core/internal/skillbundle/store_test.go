package skillbundle

import "testing"

// switchLookup answers the named switches and reads every other name as unset.
func switchLookup(values map[string]string) func(string) string {
	return func(name string) string { return values[name] }
}

// SKILL_BUNDLE_RETENTION keeps a superseded bundle only while a skill version names it, and that digest is stamped only
// while SKILL_VERSIONS_API is on. RetentionFromLookup therefore reads retention as on only when both are the literal
// "true" in one lookup, and every caller takes its answer from it.
func TestRetentionFromLookupCouplesTheTwoSwitches(t *testing.T) {
	cases := []struct {
		name      string
		retention string
		versions  string
		want      bool
	}{
		{name: "both on", retention: "true", versions: "true", want: true},
		{name: "retention on, versions off", retention: "true", versions: "", want: false},
		{name: "retention off, versions on", retention: "", versions: "true", want: false},
		{name: "both off", retention: "", versions: "", want: false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			lookup := switchLookup(map[string]string{RetentionEnv: tc.retention, skillVersionsAPIEnv: tc.versions})
			if got := RetentionFromLookup(lookup); got != tc.want {
				t.Errorf("RetentionFromLookup(%s=%q, %s=%q) = %v, want %v",
					RetentionEnv, tc.retention, skillVersionsAPIEnv, tc.versions, got, tc.want)
			}
		})
	}
}

// Both switches read the literal "true" only: any other spelling of either one leaves retention off.
func TestRetentionFromLookupReadsBothSwitchesStrictly(t *testing.T) {
	for _, value := range []string{"TRUE", "1", "yes", "on", " true"} {
		if RetentionFromLookup(switchLookup(map[string]string{RetentionEnv: value, skillVersionsAPIEnv: "true"})) {
			t.Errorf("%s=%q turned retention on with versions on", RetentionEnv, value)
		}
		if RetentionFromLookup(switchLookup(map[string]string{RetentionEnv: "true", skillVersionsAPIEnv: value})) {
			t.Errorf("%s=%q turned retention on with retention on", skillVersionsAPIEnv, value)
		}
	}
}

// skillVersionsAPIEnv is a twin of the api package's constant, which cannot be imported here, so its spelling is pinned.
func TestSkillVersionsTwinIsTheSwitchName(t *testing.T) {
	if skillVersionsAPIEnv != "SKILL_VERSIONS_API" {
		t.Fatalf("skillVersionsAPIEnv = %q, want SKILL_VERSIONS_API (the api package reads the same name)", skillVersionsAPIEnv)
	}
}
