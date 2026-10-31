package config

import (
	"strings"
	"testing"
)

func TestLoadDoffinReadsTheProcessEnvironment(t *testing.T) {
	t.Setenv("DOFFIN_API_KEY", "0123456789abcdef0123456789abcdef")
	t.Setenv("DOFFIN_SEARCH_RPM", "12")

	d, err := LoadDoffin()
	if err != nil {
		t.Fatalf("LoadDoffin: %v", err)
	}
	if !d.Configured() || d.SearchRPM != 12 {
		t.Fatalf("Doffin = %+v", d)
	}
}

func TestLoadDoffinFromReportsInvalidValuesByName(t *testing.T) {
	t.Parallel()

	_, err := LoadDoffinFrom(env(map[string]string{"DOFFIN_DOWNLOAD_RPM": "lots"}))
	if err == nil || !strings.Contains(err.Error(), "DOFFIN_DOWNLOAD_RPM") {
		t.Fatalf("error = %v, want it to name DOFFIN_DOWNLOAD_RPM", err)
	}
}

// A key pasted into the wrong variable must not be printed back by the error
// that rejects it, however short the value is.
func TestRejectedValuesAreNeverEchoed(t *testing.T) {
	t.Parallel()

	const pasted = "0123456789abcdef0123456789abcdef"
	for _, name := range []string{"PORT", "DOFFIN_SEARCH_RPM", "DOFFIN_DOWNLOAD_RPM", "DOFFIN_HTTP_TIMEOUT"} {
		_, err := LoadFrom(env(with(required(), map[string]string{name: pasted})))
		if err == nil {
			t.Fatalf("%s: expected an error", name)
		}
		if !strings.Contains(err.Error(), name) {
			t.Errorf("%s: the error should name the variable: %q", name, err)
		}
		if strings.Contains(err.Error(), pasted) {
			t.Errorf("%s: the error echoed the rejected value: %q", name, err)
		}
	}
}
