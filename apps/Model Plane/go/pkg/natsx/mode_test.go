package natsx

import "testing"

func TestParseCompatMode(t *testing.T) {
	tests := []struct {
		in   string
		want CompatMode
	}{
		{"", ModeV1Only},
		{"v1_only", ModeV1Only},
		{"dual_write", ModeDualWrite},
		{"dual_read", ModeDualRead},
		{"legacy_only", ModeLegacyOnly},
		{"garbage", ModeV1Only},
	}
	for _, tc := range tests {
		t.Run(tc.in, func(t *testing.T) {
			if got := ParseCompatMode(tc.in); got != tc.want {
				t.Errorf("ParseCompatMode(%q) = %v, want %v", tc.in, got, tc.want)
			}
		})
	}
}

func TestCompatModeString(t *testing.T) {
	tests := []struct {
		m    CompatMode
		want string
	}{
		{ModeV1Only, "v1_only"},
		{ModeDualWrite, "dual_write"},
		{ModeDualRead, "dual_read"},
		{ModeLegacyOnly, "legacy_only"},
		{CompatMode(99), "v1_only"},
	}
	for _, tc := range tests {
		if got := tc.m.String(); got != tc.want {
			t.Errorf("(%d).String() = %q, want %q", int(tc.m), got, tc.want)
		}
	}
}

func TestReadCompatModeFromEnv(t *testing.T) {
	t.Setenv("MP_COMPAT_MODE", "dual_write")
	if got := ReadCompatModeFromEnv(); got != ModeDualWrite {
		t.Errorf("ReadCompatModeFromEnv() = %v, want ModeDualWrite", got)
	}
	t.Setenv("MP_COMPAT_MODE", "")
	if got := ReadCompatModeFromEnv(); got != ModeV1Only {
		t.Errorf("ReadCompatModeFromEnv() default = %v, want ModeV1Only", got)
	}
}
