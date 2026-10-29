package doffin

import (
	"encoding/json"
	"reflect"
	"testing"
)

func TestOneOrManyDecodesArraysSingleValuesAndNull(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		in      string
		want    []string
		wantErr bool
	}{
		{"array", `["a","b"]`, []string{"a", "b"}, false},
		{"single value", `"a"`, []string{"a"}, false},
		{"null", `null`, nil, false},
		{"empty array", `[]`, []string{}, false},
		{"wrong element type", `7`, nil, true},
		{"array with a wrong element", `["a",7]`, nil, true},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			var got OneOrMany[string]
			err := json.Unmarshal([]byte(tc.in), &got)
			if (err != nil) != tc.wantErr {
				t.Fatalf("error = %v, wantErr %v", err, tc.wantErr)
			}
			if tc.wantErr {
				return
			}
			if len(got) != len(tc.want) || (len(tc.want) > 0 && !reflect.DeepEqual([]string(got), tc.want)) {
				t.Fatalf("got %v, want %v", []string(got), tc.want)
			}
		})
	}
}

func TestNumberAcceptsNumbersStringsAndNull(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		in      string
		want    string
		wantErr bool
	}{
		{"integer", `12`, "12", false},
		{"decimal", `1500000.5`, "1500000.5", false},
		{"exponent", `1.5e3`, "1.5e3", false},
		{"numeric string", `"7"`, "7", false},
		{"empty string", `""`, "", false},
		{"null", `null`, "", false},
		{"word", `"many"`, "", true},
		{"boolean", `true`, "", true},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			var got Number
			err := json.Unmarshal([]byte(tc.in), &got)
			if (err != nil) != tc.wantErr {
				t.Fatalf("error = %v, wantErr %v", err, tc.wantErr)
			}
			if !tc.wantErr && got.String() != tc.want {
				t.Fatalf("Number = %q, want %q", got.String(), tc.want)
			}
		})
	}
}

func TestCountRejectsNonFiniteAndOutOfRangeValues(t *testing.T) {
	t.Parallel()

	for _, in := range []string{
		`"NaN"`, `"Inf"`, `"-Inf"`, `"+Inf"`, `1e30`, `"1e30"`, `-1e30`,
		`9223372036854775808`, // one past MaxInt64
	} {
		var got Count
		if err := json.Unmarshal([]byte(in), &got); err == nil {
			t.Errorf("Count accepted %s as %d", in, got)
		}
	}
	for in, want := range map[string]Count{`9223372036854775807`: 9223372036854775807, `-5`: -5, `"2.0"`: 2} {
		var got Count
		if err := json.Unmarshal([]byte(in), &got); err != nil || got != want {
			t.Errorf("Count(%s) = %d, %v; want %d", in, got, err, want)
		}
	}
}
