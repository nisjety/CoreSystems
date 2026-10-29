package config

import "testing"

func TestSkipMigrationsIsExplicitAndValidated(t *testing.T) {
	for _, tc := range []struct {
		value         string
		want, invalid bool
	}{{"", false, false}, {"true", true, false}, {"false", false, false}, {"yes", false, true}, {"TRUE", false, true}} {
		t.Run(tc.value, func(t *testing.T) {
			cfg, err := LoadFrom(env(with(required(), map[string]string{"ANBUD_SKIP_MIGRATIONS": tc.value})))
			if (err != nil) != tc.invalid {
				t.Fatalf("validation = %v", err)
			}
			if err == nil && cfg.SkipMigrations != tc.want {
				t.Fatalf("skip = %v", cfg.SkipMigrations)
			}
		})
	}
}
