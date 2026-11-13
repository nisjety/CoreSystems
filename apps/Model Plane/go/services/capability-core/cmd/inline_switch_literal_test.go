package main

import (
	"os"
	"regexp"
	"testing"
)

// Plan 3.4: the switches cmd reads inline (no FromLookup parser to call) turn on only for the literal "true". Each read of
// one of them must be exactly `os.Getenv("NAME") == "true"`: no TrimSpace, no EqualFold, no `!= "false"` default-on read.
// The off paths themselves are pinned where the switch lands: Run now in internal/api/cron_run_now_test.go ("switch off"),
// the wake gate in internal/eventtrigger/wakegate_switch_test.go, and the starter's absence is the switch's whole effect.
func TestInlineSwitchesAreReadAsTheLiteralTrue(t *testing.T) {
	sources := map[string][]string{
		"CRON_RUN_NOW_ENABLED":            {"main.go"},
		"EVENT_TRIGGER_FIRE_ENABLED":      {"main.go"},
		"EVENT_TRIGGER_RUN_ENABLED":       {"event_trigger_fire.go"},
		"EVENT_TRIGGER_WAKE_GATE_ENABLED": {"main.go", "event_trigger_fire.go"},
	}
	for name, files := range sources {
		anyRead := regexp.MustCompile(`os\.Getenv\("` + name + `"\)`)
		strictRead := regexp.MustCompile(`os\.Getenv\("` + name + `"\) == "true"`)
		for _, file := range files {
			raw, err := os.ReadFile(file)
			if err != nil {
				t.Fatalf("read %s: %v", file, err)
			}
			reads := len(anyRead.FindAllIndex(raw, -1))
			strict := len(strictRead.FindAllIndex(raw, -1))
			if reads == 0 {
				t.Errorf("%s: %s is not read here any more; update this guard", file, name)
			}
			if reads != strict {
				t.Errorf("%s: %d of %d reads of %s are not the strict literal `== \"true\"`", file, reads-strict, reads, name)
			}
		}
	}
}
