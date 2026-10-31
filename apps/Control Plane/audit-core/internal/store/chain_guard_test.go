package store

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestDetailsGuardResultObservesSizeAndDeniedKeys(t *testing.T) {
	for name, tc := range map[string]struct {
		details map[string]any
		want    string
	}{
		"nil details":             {nil, "ok"},
		"documented content-free": {map[string]any{"tool": "knowledge_search", "data_category": "customer_private", "zdr": false, "phase": "final"}, "ok"},
		"counts are not tokens":   {map[string]any{"tokens_in": 12}, "ok"},
		"content key":             {map[string]any{"content": "hello"}, "denied_key"},
		"key match ignores case":  {map[string]any{"Authorization": "x"}, "denied_key"},
		"nested key":              {map[string]any{"a": map[string]any{"b": map[string]any{"prompt": "x"}}}, "denied_key"},
		"key inside an array":     {map[string]any{"a": []any{map[string]any{"secret": "x"}}}, "denied_key"},
		"oversize":                {map[string]any{"blob": strings.Repeat("x", detailsGuardMaxBytes)}, "oversize"},
	} {
		encoded, err := json.Marshal(tc.details)
		if err != nil {
			t.Fatal(err)
		}
		if got := detailsGuardResult(tc.details, encoded); got != tc.want {
			t.Errorf("%s: result = %s, want %s", name, got, tc.want)
		}
	}
}
