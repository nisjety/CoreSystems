package validate

import (
	"strings"
	"testing"

	"github.com/triodelab/dataplane/services/documents-api-go/internal/model"
)

// validBaseInput returns a CreateDocumentInput that passes every validation
// rule except whatever the caller mutates. Used to isolate one field at a time.
func validBaseInput() *model.CreateDocumentInput {
	return &model.CreateDocumentInput{
		OrgID:   "org_test",
		Source:  "quarry",
		Type:    "web_page",
		Title:   "Example",
		Content: "some body content",
	}
}

// TestCreateDocument_ZDRClassificationRoundTrip pins the receiving-boundary
// contract that Quarry-v2's ingest client now relies on: the values its
// PrivacyClassification → zdr_classification mapping emits must be accepted
// here (otherwise the wire hop would 400 and the policy would never persist),
// and the old ZdrMode-derived "standard"/"ephemeral" values — which the wire
// never sends on the durable path — must be rejected so a regression that
// re-introduces them is caught.
func TestCreateDocument_ZDRClassificationRoundTrip(t *testing.T) {
	cases := []struct {
		name           string
		classification string
		wantErr        bool
	}{
		// Every value Quarry's zdr_classification_for() can emit.
		{"public", "public", false},
		{"internal", "internal", false},
		{"sensitive", "sensitive", false},
		{"restricted", "restricted", false},
		// Empty is allowed and interpreted as "internal" downstream.
		{"empty defaults internal", "", false},
		// Values the durable wire path must never send.
		{"legacy grpc standard rejected", "standard", true},
		{"legacy grpc ephemeral rejected", "ephemeral", true},
		{"garbage rejected", "not-a-class", true},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			in := validBaseInput()
			in.ZDRClassification = tc.classification
			err := CreateDocument(in)
			if tc.wantErr && err == nil {
				t.Fatalf("expected error for zdr_classification=%q, got nil", tc.classification)
			}
			if !tc.wantErr && err != nil {
				t.Fatalf("expected no error for zdr_classification=%q, got %v", tc.classification, err)
			}
			if tc.wantErr && err != nil && !strings.Contains(err.Error(), "zdr_classification") {
				t.Fatalf("expected a zdr_classification error, got %v", err)
			}
		})
	}
}

// TestCreateDocument_ContentRequired guards the invariant the finspo/GitHub/Slack
// content-sync work depends on: a document with no body is rejected at the
// boundary (so a metadata-only forward can never silently create an empty doc).
func TestCreateDocument_ContentRequired(t *testing.T) {
	in := validBaseInput()
	in.Content = ""
	if err := CreateDocument(in); err == nil {
		t.Fatal("expected content-required error, got nil")
	}
}
