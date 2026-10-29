package match

// Passage is a piece of a tender document's text and where it is: a page of a PDF, or a paragraph
// of any other kind of file. Exactly one of Page and Paragraph is set, counting from 1.
type Passage struct {
	Page      int
	Paragraph int
	Text      string
}

// FindingClass says what a passage that names a capability says about it.
type FindingClass string

const (
	// ClassRequirement: the passage asks for it.
	ClassRequirement FindingClass = "requirement"
	// ClassAward: the passage rewards it in the evaluation. It is not a condition for bidding.
	ClassAward FindingClass = "award"
	// ClassNegated: the passage says it is not required.
	ClassNegated FindingClass = "negated"
	// ClassMention: the passage names it and says no more.
	ClassMention FindingClass = "mention"
)

// Finding is one passage of one document that names one capability, found by keyword. It is
// evidence for a person to read, not a fact: the class is a heuristic, and the snippet is the
// sentence it was judged on.
type Finding struct {
	Capability Capability
	Class      FindingClass
	Document   string // the file's path in the uploaded archive
	Page       int
	Paragraph  int
	Snippet    string
}

// Evidence is what the tender documents a company uploaded for one notice say. The zero value is no
// documents.
type Evidence struct {
	Documents int // uploaded documents that were read
	Unread    int // uploaded documents that could not be read: scans, damaged files, kinds that are not read
	// Findings are in document order, and in page or paragraph order within a document.
	Findings []Finding
	// FindingsDropped is how many distinct findings of the uploaded documents are not in Findings, because the evidence of a
	// notice holds only the best of them: len(Findings) and this are all of them.
	FindingsDropped int
}
