package flagregistry

import (
	"strings"
	"testing"
)

// W3-FIX-8: the Space listing refusal is on by default, so the shipped entry must say
// so in every place that could drift: the default, the code default, the reason it is
// on, and both declarations the Model Plane compose sets.
func TestOwnerListSpaceRefuseShipsOnByDefault(t *testing.T) {
	reg := loadShipped(t)
	flag, ok := reg.Flag("OWNER_LIST_SPACE_REFUSE")
	if !ok {
		t.Fatal("OWNER_LIST_SPACE_REFUSE is not in the shipped registry")
	}
	if flag.Default != "true" {
		t.Fatalf("default = %q, want true", flag.Default)
	}
	if flag.CodeDefault != "true" {
		t.Fatalf("codeDefault = %q, want true", flag.CodeDefault)
	}
	if strings.TrimSpace(flag.OnDefaultReason) == "" {
		t.Fatal("onDefaultReason is empty; a switch that is on by default must say why")
	}
	for _, service := range []string{"model-gateway", "execution-core"} {
		found := false
		for _, declaration := range flag.Declarations {
			if declaration.Service != service || declaration.Env != "OWNER_LIST_SPACE_REFUSE" {
				continue
			}
			found = true
			if declaration.Default != "true" {
				t.Fatalf("%s declares default %q, want true", service, declaration.Default)
			}
		}
		if !found {
			t.Fatalf("no OWNER_LIST_SPACE_REFUSE declaration for %s", service)
		}
	}
}
