package main

import "testing"

// F-f03-03: BROWSER_SPACE_FLOOR_ENABLED is off unless it is the exact literal "true", read once into the server options.
func TestSpaceFloorSwitchReadsTheExactLiteralTrue(t *testing.T) {
	for value, want := range map[string]bool{
		"true":  true,
		"":      false,
		"TRUE":  false,
		"True":  false,
		"1":     false,
		"on":    false,
		" true": false,
		"yes":   false,
	} {
		options, err := serverOptionsFromEnv(func(name string) string {
			if name == envSpaceFloor {
				return value
			}
			return ""
		})
		if err != nil {
			t.Fatalf("serverOptionsFromEnv(%s=%q): %v", envSpaceFloor, value, err)
		}
		if options.SpaceFloor != want {
			t.Errorf("%s=%q read as %v, want %v", envSpaceFloor, value, options.SpaceFloor, want)
		}
	}
}
