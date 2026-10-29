package eforms

import (
	"strings"
	"testing"
)

// selectionCriteria is the extension block of a lot's tendering terms, as the captured notices lay
// it out: each criterion has a type code or a type, a usage code, and a name and description in
// Norwegian and English. The text is invented.
const selectionCriteria = `<cac:TenderingTerms>
      <ext:UBLExtensions><ext:UBLExtension><ext:ExtensionContent><efext:EformsExtension>
        <efac:SelectionCriteria xmlns:efbc="http://data.europa.eu/p27/eforms-ubl-extension-basic-components/1">
          <cbc:CalculationExpressionCode listName="usage">used</cbc:CalculationExpressionCode>
          <cbc:TendererRequirementTypeCode listName="selection-criterion">slc-abil-ref-services</cbc:TendererRequirementTypeCode>
          <cbc:Name languageID="ENG">References</cbc:Name>
          <cbc:Name languageID="NOR">Referanser</cbc:Name>
          <cbc:Description languageID="ENG">Three references.</cbc:Description>
          <cbc:Description languageID="NOR">Tre referanser fra tilsvarende oppdrag.</cbc:Description>
        </efac:SelectionCriteria>
        <efac:SelectionCriteria>
          <cbc:CalculationExpressionCode listName="usage">used</cbc:CalculationExpressionCode>
          <cbc:CriterionTypeCode listName="selection-criterion">other</cbc:CriterionTypeCode>
          <cbc:Name languageID="NOR">Kvalitetssikring</cbc:Name>
          <cbc:Description languageID="NOR">Leverandøren skal ha ISO 9001 eller tilsvarende.</cbc:Description>
        </efac:SelectionCriteria>
        <efac:SelectionCriteria>
          <cbc:CalculationExpressionCode listName="usage">not-used</cbc:CalculationExpressionCode>
          <cbc:TendererRequirementTypeCode listName="selection-criterion">slc-stand-other</cbc:TendererRequirementTypeCode>
          <cbc:Name languageID="NOR">Ikke brukt</cbc:Name>
        </efac:SelectionCriteria>
      </efext:EformsExtension></ext:ExtensionContent></ext:UBLExtension></ext:UBLExtensions>
      <cac:CallForTendersDocumentReference>`

func TestParseReadsTheSelectionCriteriaOfALot(t *testing.T) {
	t.Parallel()

	n, err := Parse(sample("<cac:TenderingTerms>\n      <cac:CallForTendersDocumentReference>", selectionCriteria))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	want := []SelectionCriterion{
		{Lot: "LOT-0001", Type: "slc-abil-ref-services", Name: "Referanser", Description: "Tre referanser fra tilsvarende oppdrag."},
		{Lot: "LOT-0001", Type: "other", Name: "Kvalitetssikring", Description: "Leverandøren skal ha ISO 9001 eller tilsvarende."},
	}
	if len(n.SelectionCriteria) != len(want) {
		t.Fatalf("criteria = %+v, want %d (a criterion marked not-used is left out)", n.SelectionCriteria, len(want))
	}
	for i := range want {
		if n.SelectionCriteria[i] != want[i] {
			t.Errorf("criterion %d = %+v\nwant %+v", i, n.SelectionCriteria[i], want[i])
		}
	}
	for _, path := range n.Unmapped {
		if path == "ContractNotice/ProcurementProjectLot/TenderingTerms/UBLExtensions" {
			t.Errorf("the selection criteria block is reported unread")
		}
	}
}

func TestParseFallsBackToTheFirstLanguage(t *testing.T) {
	t.Parallel()

	doc := sample("<cac:TenderingTerms>\n      <cac:CallForTendersDocumentReference>", selectionCriteria)
	n, err := Parse([]byte(strings.ReplaceAll(string(doc), `languageID="NOR"`, `languageID="SWE"`)))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if got := n.SelectionCriteria[0].Name; got != "References" {
		t.Errorf("name = %q, want the first one given when there is no Norwegian", got)
	}
}
