package eforms

import (
	"slices"
	"testing"
)

// ExternalLinks gives every link a notice publishes in an external reference, wherever it sits in
// the notice. It is a measurement of what buyers link, not the parser's model of the notice, so it
// takes no view of which link is a tender document.
func TestExternalLinksFindsEveryExternalReference(t *testing.T) {
	t.Parallel()

	links, err := ExternalLinks(sample())
	if err != nil {
		t.Fatalf("ExternalLinks: %v", err)
	}
	if !slices.Equal(links, []string{"https://example.invalid/docs/1"}) {
		t.Errorf("links = %v, want the one link of the sample", links)
	}
}

// A URI that is not inside an external reference is not a link to a document, and is not returned.
func TestExternalLinksIgnoresOtherURIs(t *testing.T) {
	t.Parallel()

	links, err := ExternalLinks(sample("<cac:Attachment>", "<cac:Attachment><cbc:URI>https://example.invalid/other</cbc:URI>"))
	if err != nil {
		t.Fatalf("ExternalLinks: %v", err)
	}
	if !slices.Equal(links, []string{"https://example.invalid/docs/1"}) {
		t.Errorf("links = %v, want only the one inside an external reference", links)
	}
}
