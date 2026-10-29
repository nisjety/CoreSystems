package config

import (
	"strings"
	"testing"
	"time"
)

func TestTenderDocsAreOffByDefault(t *testing.T) {
	t.Parallel()

	cfg, err := LoadFrom(env(required()))
	if err != nil {
		t.Fatalf("LoadFrom: %v", err)
	}
	// The feature keeps a company's own documents, so it is an operator's decision and not a default.
	if cfg.TenderDocs.Enabled {
		t.Error("tender documents are on with nothing set")
	}
	if cfg.TenderDocs.Retention != 90*24*time.Hour || cfg.TenderDocs.MaxArchiveBytes != 100<<20 {
		t.Errorf("defaults = %+v, want 90 days and 100 MiB", cfg.TenderDocs)
	}
}

func TestTenderDocsAreSwitchedOnAndOffByName(t *testing.T) {
	t.Parallel()

	for value, want := range map[string]bool{
		"true": true, "TRUE": true, "True": true, "1": true, " true ": true,
		"false": false, "FALSE": false, "0": false, "": false,
	} {
		cfg, err := LoadFrom(env(with(required(), map[string]string{"ANBUD_TENDER_DOCS": value})))
		if err != nil || cfg.TenderDocs.Enabled != want {
			t.Errorf("ANBUD_TENDER_DOCS=%q: enabled %v, err %v; want %v", value, cfg.TenderDocs.Enabled, err, want)
		}
	}
}

func TestTenderDocsLoadTheirLimits(t *testing.T) {
	t.Parallel()

	cfg, err := LoadFrom(env(with(required(), map[string]string{
		"ANBUD_TENDER_DOCS": "true", "ANBUD_TENDER_DOCS_RETENTION": "720h", "ANBUD_TENDER_DOCS_MAX_MB": "250",
	})))
	if err != nil {
		t.Fatalf("LoadFrom: %v", err)
	}
	if !cfg.TenderDocs.Enabled || cfg.TenderDocs.Retention != 720*time.Hour || cfg.TenderDocs.MaxArchiveBytes != 250<<20 {
		t.Errorf("TenderDocs = %+v", cfg.TenderDocs)
	}
}

// A mistake stops the service at start-up and names its variable, whether or not the feature is on:
// a setting that would have been wrong the day somebody turns it on is wrong now.
func TestTenderDocsRefuseValuesThatCannotWorkAndNeverEchoThem(t *testing.T) {
	t.Parallel()

	const pasted = "0123456789abcdef0123456789abcdef"
	for name, vars := range map[string]map[string]string{
		"switch that is not a bool":     {"ANBUD_TENDER_DOCS": pasted},
		"yes is not a bool":             {"ANBUD_TENDER_DOCS": "yes"},
		"retention that is not a time":  {"ANBUD_TENDER_DOCS_RETENTION": pasted},
		"retention of no time":          {"ANBUD_TENDER_DOCS_RETENTION": "0s"},
		"retention before the creation": {"ANBUD_TENDER_DOCS_RETENTION": "-24h"},
		"retention over two years":      {"ANBUD_TENDER_DOCS_RETENTION": "17521h"},
		"retention under a second":      {"ANBUD_TENDER_DOCS_RETENTION": "999ms"},
		"retention of a nanosecond":     {"ANBUD_TENDER_DOCS_RETENTION": "1ns"},
		"days are not a duration":       {"ANBUD_TENDER_DOCS_RETENTION": "90d"},
		"size that is not a number":     {"ANBUD_TENDER_DOCS_MAX_MB": pasted},
		"size of nothing":               {"ANBUD_TENDER_DOCS_MAX_MB": "0"},
		"negative size":                 {"ANBUD_TENDER_DOCS_MAX_MB": "-5"},
		"size over what memory allows":  {"ANBUD_TENDER_DOCS_MAX_MB": "513"},
	} {
		_, err := LoadFrom(env(with(required(), vars)))
		if err == nil {
			t.Errorf("%s: accepted", name)
			continue
		}
		if !strings.Contains(err.Error(), "ANBUD_TENDER_DOCS") {
			t.Errorf("%s: the error does not name the variable: %v", name, err)
		}
		if strings.Contains(err.Error(), pasted) {
			t.Errorf("%s: the error echoes the value: %v", name, err)
		}
	}
}

func TestTenderDocsAllowTwoYearsAndNoMore(t *testing.T) {
	t.Parallel()

	for value, ok := range map[string]bool{"17520h": true, "17521h": false, "1h": true, "1s": true, "999ms": false, "1ns": false} {
		_, err := LoadFrom(env(with(required(), map[string]string{"ANBUD_TENDER_DOCS_RETENTION": value})))
		if (err == nil) != ok {
			t.Errorf("retention %s: err = %v, want accepted = %v", value, err, ok)
		}
	}
	for value, ok := range map[string]bool{"1": true, "512": true, "513": false} {
		_, err := LoadFrom(env(with(required(), map[string]string{"ANBUD_TENDER_DOCS_MAX_MB": value})))
		if (err == nil) != ok {
			t.Errorf("max MB %s: err = %v, want accepted = %v", value, err, ok)
		}
	}
}
