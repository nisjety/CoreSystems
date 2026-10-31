package eforms

import (
	"github.com/triodelab/anbud/internal/eforms/eformstest"
	"strings"
	"testing"
)

func TestContractingSystemsReadSourceCodesByNamespaceAndList(t *testing.T) {
	raw := string(eformstest.ContractNotice(eformstest.Notice{Title: "A contract", CPV: "72000000"}))
	raw = strings.Replace(raw, "<cac:TenderingProcess>", `<cac:TenderingProcess><cac:ContractingSystem><cbc:ContractingSystemTypeCode listName="framework-agreement">fa-wo-rc</cbc:ContractingSystemTypeCode></cac:ContractingSystem><cac:ContractingSystem><cbc:ContractingSystemTypeCode listName="dps-usage">none</cbc:ContractingSystemTypeCode></cac:ContractingSystem>`, 1)
	n, err := Parse([]byte(raw))
	if err != nil {
		t.Fatal(err)
	}
	if len(n.ContractingSystems) != 2 || n.ContractingSystems[0].Kind != "framework-agreement" || n.ContractingSystems[0].Code != "fa-wo-rc" {
		t.Fatalf("systems=%+v", n.ContractingSystems)
	}
}
