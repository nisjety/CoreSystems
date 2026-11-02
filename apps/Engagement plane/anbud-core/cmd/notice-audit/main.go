// notice-audit reads held-notice NDJSON from stdin and reports counts only.
// Source bodies, identifiers, descriptions and contact details are never printed.
package main

import (
	"encoding/json"
	"errors"
	"fmt"
	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/sources/doffin"
	"io"
	"os"
)

func main() {
	counts := map[string]int{}
	shapes := map[string]int{}
	missing := map[string]int{}
	decoder := json.NewDecoder(os.Stdin)
	for {
		var doc doffin.Notice
		err := decoder.Decode(&doc)
		if err == io.EOF {
			break
		}
		if err != nil {
			fmt.Fprintln(os.Stderr, "notice-audit: invalid input")
			os.Exit(1)
		}
		counts["stored"]++
		header, err := eforms.Sniff(doc.Body)
		if err != nil {
			counts["unreadable"]++
			continue
		}
		shape := header.Root + "/" + header.SDKVersion + "/" + string(header.Flavour)
		if header.CustomizationID == "eforms-sdk-"+header.SDKVersion {
			shape += "standard"
		}
		shapes[shape]++
		n, err := eforms.Parse(doc.Body)
		if errors.Is(err, eforms.ErrNoFieldMap) {
			missing[shape]++
			continue
		}
		if err != nil {
			counts["unreadable"]++
			continue
		}
		counts["parsed"]++
		if n.Title != "" {
			counts["title"]++
		}
		if n.Description != "" {
			counts["description"]++
		}
		if len(n.BuyerNames) > 0 {
			counts["buyer"]++
		}
		if len(n.Documents) > 0 {
			counts["document_links"]++
		}
		if len(n.SelectionCriteria) > 0 {
			counts["qualification_criteria"]++
		}
		if len(n.Requirements) > 0 {
			counts["requirements_or_exclusions"]++
		}
		if len(n.Results) > 0 {
			counts["award_results"]++
		}
		if n.Change != nil {
			counts["changes"]++
		}
		hasCriteria, hasPeriod := false, false
		for _, lot := range n.Lots {
			hasCriteria = hasCriteria || len(lot.AwardCriteria) > 0
			hasPeriod = hasPeriod || lot.Period != nil
		}
		if hasCriteria {
			counts["evaluation_criteria"]++
		}
		if hasPeriod {
			counts["contract_terms"]++
		}
	}
	if err := json.NewEncoder(os.Stdout).Encode(map[string]any{"coverage": counts, "shapes": shapes, "unsupported_shapes": missing}); err != nil {
		os.Exit(1)
	}
}
