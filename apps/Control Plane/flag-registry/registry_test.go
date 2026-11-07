package flagregistry

import (
	"strings"
	"testing"
)

func exampleSwitch() Flag {
	return Flag{
		Name:         "EXAMPLE_ENABLED",
		Kind:         KindSwitch,
		Owner:        PlaneModel,
		Source:       SourceCompose,
		Default:      "false",
		Purpose:      "Example switch for the registry tests.",
		Declarations: []Declaration{{Service: "model-gateway", Env: "EXAMPLE_ENABLED", Default: "false"}},
	}
}

func exampleFile(flags ...Flag) File {
	return File{Schema: SchemaV1, Flags: flags}
}

func TestNewAcceptsAValidDocument(t *testing.T) {
	reg, err := New(exampleFile(exampleSwitch()))
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	if _, ok := reg.Flag("EXAMPLE_ENABLED"); !ok {
		t.Fatal("registered flag not found")
	}
}

func TestNewAcceptsACodeFlagThatComposeAlsoSets(t *testing.T) {
	f := Flag{
		Name:         "SHARED_DEV_ENABLED",
		Kind:         KindSwitch,
		Owner:        PlaneFrontend,
		Source:       SourceCode,
		Default:      "false",
		Purpose:      "Dev switch that another plane's compose also sets blank.",
		CodePath:     "apps/Frontend Plane/README.md",
		Declarations: []Declaration{{Service: "model-gateway", Env: "SHARED_DEV_ENABLED", Default: ""}},
	}
	if _, err := New(exampleFile(f)); err != nil {
		t.Fatalf("New: %v", err)
	}
}

func TestNewRejectsInvalidDocuments(t *testing.T) {
	mutate := func(edit func(*Flag)) File {
		f := exampleSwitch()
		edit(&f)
		return exampleFile(f)
	}
	outOfOrder := func() File {
		a := exampleSwitch()
		a.Name = "EXAMPLE_B_ENABLED"
		a.Declarations = []Declaration{{Service: "model-gateway", Env: "EXAMPLE_B_ENABLED", Default: "false"}}
		b := exampleSwitch()
		b.Name = "EXAMPLE_A_ENABLED"
		b.Declarations = []Declaration{{Service: "model-gateway", Env: "EXAMPLE_A_ENABLED", Default: "false"}}
		return exampleFile(a, b)
	}
	cases := []struct {
		name string
		file File
		want string
	}{
		{"wrong schema", File{Schema: "other/v9"}, "schema"},
		{"lowercase name", mutate(func(f *Flag) { f.Name = "example_enabled" }), "name must match"},
		{"unknown owner", mutate(func(f *Flag) { f.Owner = "billing-plane" }), "unknown owner"},
		{"unknown kind", mutate(func(f *Flag) { f.Kind = "enum" }), "unknown kind"},
		{"unknown source", mutate(func(f *Flag) { f.Source = "env" }), "unknown source"},
		{"empty purpose", mutate(func(f *Flag) { f.Purpose = "" }), "purpose must be"},
		{"padded purpose", mutate(func(f *Flag) { f.Purpose = " padded " }), "purpose must be"},
		{"compose flag without declarations", mutate(func(f *Flag) { f.Declarations = nil }), "at least one declaration"},
		{"declaration default differs", mutate(func(f *Flag) { f.Declarations[0].Default = "true" }), "defaults to"},
		{"duplicate declaration", mutate(func(f *Flag) { f.Declarations = append(f.Declarations, f.Declarations[0]) }), "duplicate declaration"},
		{"bad declaration service", mutate(func(f *Flag) { f.Declarations[0].Service = "Model_Gateway" }), "is not a compose service name"},
		{"code flag declared with another default", mutate(func(f *Flag) {
			f.Source = SourceCode
			f.CodePath = "a/b.rs"
			f.Declarations[0].Default = "true"
		}), "defaults to"},
		{"code flag without codePath", mutate(func(f *Flag) { f.Source = SourceCode; f.Declarations = nil }), "need a codePath"},
		{"codePath escapes the repository", mutate(func(f *Flag) { f.CodePath = "../secrets.rs" }), "codePath"},
		{"codePath uses backslashes", mutate(func(f *Flag) { f.CodePath = `apps\x.rs` }), "codePath"},
		{"codeDefault on a code flag", mutate(func(f *Flag) {
			f.Source = SourceCode
			f.Declarations = nil
			f.CodePath = "a/b.rs"
			f.CodeDefault = "true"
		}), "applies only to compose flags"},
		{"switch default is not a switch value", mutate(func(f *Flag) {
			f.Default = "maybe"
			f.Declarations[0].Default = "maybe"
		}), "default: unknown switch value"},
		{"switch on by default without a reason", mutate(func(f *Flag) {
			f.Default = "true"
			f.Declarations[0].Default = "true"
		}), "onDefaultReason"},
		{"reason given while off", mutate(func(f *Flag) { f.OnDefaultReason = "Why." }), "onDefaultReason is set but the default is off"},
		{"code default differs without a drift reason", mutate(func(f *Flag) { f.CodeDefault = "true" }), "driftReason"},
		{"drift reason while code matches", mutate(func(f *Flag) {
			f.CodeDefault = "false"
			f.DriftReason = "Why."
		}), "driftReason is set but"},
		{"drift reason without a code default", mutate(func(f *Flag) { f.DriftReason = "Why." }), "driftReason needs a codeDefault"},
		{"code default is not a switch value", mutate(func(f *Flag) { f.CodeDefault = "sometimes" }), "codeDefault: unknown switch value"},
		{"dev-only switch on by default", mutate(func(f *Flag) {
			f.DevOnly = true
			f.Default = "true"
			f.Declarations[0].Default = "true"
			f.OnDefaultReason = "Why."
		}), "devOnly flags must default off"},
		{"choice without values", mutate(func(f *Flag) { f.Kind = KindChoice }), "need values"},
		{"choice with a duplicate value", mutate(func(f *Flag) {
			f.Kind = KindChoice
			f.Values = []string{"off", "off"}
			f.Default = "off"
			f.Declarations[0].Default = "off"
		}), "duplicate value"},
		{"choice default outside its values", mutate(func(f *Flag) {
			f.Kind = KindChoice
			f.Values = []string{"on", "strict"}
			f.Default = "off"
			f.Declarations[0].Default = "off"
		}), "is not one of the values"},
		{"text flag with values", mutate(func(f *Flag) {
			f.Kind = KindText
			f.Values = []string{"a"}
		}), "text flags take no values"},
		{"choice carrying a switch-only attribute", mutate(func(f *Flag) {
			f.Kind = KindChoice
			f.Values = []string{"false"}
			f.DevOnly = true
		}), "only switch flags take"},
		{"flags out of order", outOfOrder(), "sorted by name"},
		{"duplicate flag name", exampleFile(exampleSwitch(), exampleSwitch()), "sorted by name"},
		{"unregistered name off the convention", File{Schema: SchemaV1, Flags: []Flag{exampleSwitch()}, Unregistered: []string{"DREAMING_LLM_EXTRACTION"}}, "naming convention"},
		{"unregistered name also registered", File{Schema: SchemaV1, Flags: []Flag{exampleSwitch()}, Unregistered: []string{"EXAMPLE_ENABLED"}}, "both registered and unregistered"},
		{"unregistered names out of order", File{Schema: SchemaV1, Unregistered: []string{"B_ENABLED", "A_ENABLED"}}, "unregistered names must be sorted"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			_, err := New(tc.file)
			if err == nil {
				t.Fatalf("New accepted the document; want an error containing %q", tc.want)
			}
			if !strings.Contains(err.Error(), tc.want) {
				t.Fatalf("error %q does not mention %q", err, tc.want)
			}
		})
	}
}

func TestLoadRejectsMalformedDocuments(t *testing.T) {
	cases := map[string]string{
		"unknown top-level field": `{"schema":"verevon.flag-registry/v1","flags":[],"unregistered":[],"extra":1}`,
		"unknown flag field":      `{"schema":"verevon.flag-registry/v1","flags":[{"name":"A_ENABLED","kind":"switch","owner":"model-plane","source":"code","codePath":"x/y.rs","default":"false","purpose":"p","colour":"red"}],"unregistered":[]}`,
		"data after the document": `{"schema":"verevon.flag-registry/v1","flags":[],"unregistered":[]} {}`,
		"empty input":             ``,
	}
	for name, doc := range cases {
		t.Run(name, func(t *testing.T) {
			if _, err := Load(strings.NewReader(doc)); err == nil {
				t.Fatal("Load accepted a malformed document")
			}
		})
	}
}

func TestLoadAcceptsAnEmptyRegistryAndTrailingWhitespace(t *testing.T) {
	doc := `{"schema":"verevon.flag-registry/v1","flags":[],"unregistered":[]}` + "\n\n"
	reg, err := Load(strings.NewReader(doc))
	if err != nil {
		t.Fatalf("Load: %v", err)
	}
	if n := len(reg.Flags()); n != 0 {
		t.Fatalf("Flags() returned %d entries, want 0", n)
	}
}

func TestRegistryReturnsCopies(t *testing.T) {
	reg, err := New(exampleFile(exampleSwitch()))
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	f, _ := reg.Flag("EXAMPLE_ENABLED")
	f.Declarations[0].Service = "tampered"
	again, _ := reg.Flag("EXAMPLE_ENABLED")
	if again.Declarations[0].Service != "model-gateway" {
		t.Fatalf("mutating a returned flag changed the registry: %+v", again.Declarations)
	}
}
