package tenderdocs

import (
	"context"
	"errors"
	"slices"
	"strings"

	"github.com/triodelab/anbud/internal/docread"
	"github.com/triodelab/anbud/internal/docstore"
	"github.com/triodelab/anbud/internal/match"
	"github.com/triodelab/anbud/internal/ziputil"
)

// perCapability is how many findings of one capability an upload keeps. A large package says
// "ESPD" on every page of its procedure, and those must not push the one sentence that asks for a
// certificate out of the list.
const perCapability = 50

// unnamed stands for a file whose name was unusable and so is not known, in the manifest.
const unnamed = "(unnamed)"

// processed is what an archive came to: its manifest and the findings that are kept.
type processed struct {
	documents []docstore.Document
	findings  []docstore.Finding
	dropped   int // findings left out because an upload keeps only so many, or only so many bytes of them
}

// process reads every file of the archive, one at a time. The text of a file lives as long as the
// call that reads it.
func (s *Service) process(ctx context.Context, archive []byte) (processed, error) {
	w := &walk{svc: s, seen: map[string]bool{}}
	report, err := ziputil.Walk(ctx, archive, s.limits, func(e ziputil.Entry) error { return w.entry(ctx, e) })
	if err != nil {
		return processed{}, archiveError(err)
	}
	w.finish()
	for _, sk := range report.Skipped {
		w.docs = append(w.docs, skippedDocument(sk))
	}
	// A file that was skipped and does not fit in the list is a file the manifest would not show: an
	// archive with that many unusable files is refused, so that nothing is lost without a number.
	if len(w.docs) > docstore.MaxDocuments || report.SkippedMore > 0 {
		return processed{}, &ArchiveError{Reason: ReasonTooManyFiles}
	}
	findings, tooLarge := fit(toStored(w.found), docstore.MaxEncodedBytes)
	return processed{documents: w.docs, findings: findings, dropped: w.dropped + tooLarge}, nil
}

// archiveError turns what the archive reader refused into the reason the caller is given. What it
// does not know is passed on as it is: the end of the context, and an ArchiveError that the walk's own
// callback returned.
func archiveError(err error) error {
	switch {
	case errors.Is(err, ziputil.ErrNotArchive):
		return &ArchiveError{Reason: ReasonNotAZip}
	case errors.Is(err, ziputil.ErrTooLarge):
		return &ArchiveError{Reason: ReasonTooLarge}
	case errors.Is(err, ziputil.ErrTooManyEntries):
		return &ArchiveError{Reason: ReasonTooManyEntries}
	case errors.Is(err, ziputil.ErrBomb):
		return &ArchiveError{Reason: ReasonExpandsTooFar}
	}
	return err
}

// walk is the state of reading one archive.
type walk struct {
	svc  *Service
	seen map[string]bool // hashes of the files read so far
	docs []docstore.Document
	// found is what is kept of the findings so far, in the order of the archive. It is cut to what an upload
	// may keep whenever it has grown to twice that, and when the walk is done, so that a file that names a
	// certificate in every sentence, or a great many of them, never holds more than that and a passage's worth.
	found   []match.Finding
	dropped int // the findings that were left out, in all
}

// entry reads one file and adds it to the manifest. It fails only when the archive is to be refused or
// the context has ended: whatever is wrong with the file itself is its status.
func (w *walk) entry(ctx context.Context, e ziputil.Entry) error {
	if len(w.docs) >= docstore.MaxDocuments {
		return &ArchiveError{Reason: ReasonTooManyFiles}
	}
	kind := docread.Detect(e.Data)
	doc := docstore.Document{Path: e.Path, Kind: string(kind), Size: e.Size, SHA256: e.SHA256}

	if w.seen[e.SHA256] {
		doc.Status = docstore.StatusDuplicate
		w.docs = append(w.docs, doc)
		return nil
	}
	w.seen[e.SHA256] = true

	c, err := w.svc.safeRead(ctx, e.Data)
	if err := ctx.Err(); err != nil {
		return err
	}
	if c.Kind != "" {
		doc.Kind = string(c.Kind)
	}
	switch {
	case err == nil:
		doc.Pages, doc.Chars, doc.Truncated, doc.Extractor = c.Pages, c.Chars, c.Truncated, c.Extractor
		if len(c.Units) == 0 {
			doc.Status = docstore.StatusEmpty // read, and there is no text in it: a scan
			break
		}
		doc.Status = docstore.StatusRead
		for _, u := range c.Units {
			w.addPassage(e.Path, match.Passage{Page: u.Page, Paragraph: u.Paragraph, Text: u.Text})
		}
	case errors.Is(err, docread.ErrUnsupported):
		doc.Status = docstore.StatusUnsupported
	case errors.Is(err, docread.ErrEncrypted):
		doc.Status = docstore.StatusEncrypted
	case errors.Is(err, docread.ErrTooLarge):
		doc.Status = docstore.StatusTooLarge
	case errors.Is(err, docread.ErrMalformed):
		doc.Status, doc.Reason = docstore.StatusFailed, "damaged"
	case errors.Is(err, docread.ErrTimeout):
		doc.Status, doc.Reason = docstore.StatusFailed, "timeout"
	default:
		doc.Status, doc.Reason = docstore.StatusFailed, "reader_error"
	}
	w.docs = append(w.docs, doc)
	return nil
}

// cutAbove is how many findings a walk holds before it cuts them to what an upload keeps. Cutting is not free (it
// ranks what it holds), and it can be put off: the cut can be applied again to what it returned and what came
// after, with the same result as once to everything.
const cutAbove = 2 * docstore.MaxFindings

// addPassage adds what one page or paragraph of a document names to what is kept. The finder judges each
// passage alone, so this finds what finding in the whole file would. What is kept is cut whenever it has
// grown to twice what an upload keeps, so a file that names a certificate in every sentence, or a great many in
// one, never holds more than that and a passage's worth beyond it; the walk cuts it a last time when it is done
// (finish).
func (w *walk) addPassage(document string, p match.Passage) {
	w.found = append(w.found, match.FindInPassages(document, []match.Passage{p})...)
	if len(w.found) > cutAbove {
		w.finish()
	}
}

// finish cuts what is kept to what an upload keeps, and counts what was left out.
func (w *walk) finish() {
	var dropped int
	w.found, dropped = selectWithin(w.found, perCapability, docstore.MaxFindings)
	w.dropped += dropped
}

// safeRead reads one document and never lets the reader take the service down: a file is from a
// stranger, and a reader that panics on it has not read it. What panicked is not logged, because the
// value of a panic can hold what the reader was reading.
func (s *Service) safeRead(ctx context.Context, data []byte) (c docread.Content, err error) {
	defer func() {
		if recover() != nil {
			s.log.Error().Msg("tender documents: a document reader panicked; the file is listed as unreadable")
			c, err = docread.Content{}, docread.ErrFailed
		}
	}()
	return s.reader.Read(ctx, data)
}

// skippedDocument lists a file that was not delivered, with the reason. It was never read, so it has
// no hash, and a file whose name was unusable is listed as (unnamed).
func skippedDocument(sk ziputil.Skipped) docstore.Document {
	path := strings.TrimSpace(sk.Path)
	if path == "" {
		path = unnamed
	}
	return docstore.Document{Path: path, Kind: string(docread.KindOther), Status: docstore.StatusSkipped, Reason: string(sk.Reason)}
}

// classRank orders what matters for the gate: what asks for a capability, then what says it is not
// asked for, then what only rewards it, then what only names it.
var classRank = map[match.FindingClass]int{
	match.ClassRequirement: 0, match.ClassNegated: 1, match.ClassAward: 2, match.ClassMention: 3,
}

// selectWithin keeps at most perCapability findings of each capability, and at most total in all: the
// ones that matter most, and of equal weight the earliest. They are returned in the order they came in.
// The count says how many were left out, so that nothing is lost without a number. It can be applied again
// to what it returned and what came after, with the same result as once to everything: a finding is left
// out only when that many that matter more are there, and they stay.
//
// With the rules as they are the limit on the whole can never bind, because eight capabilities of 50 make
// exactly the 400 an upload may keep; it is there for the day a rule is added, and is tested with limits
// that make it bind.
func selectWithin(all []match.Finding, perCapability, total int) (kept []match.Finding, dropped int) {
	byCapability := map[match.Capability][]int{}
	for i, f := range all {
		byCapability[f.Capability] = append(byCapability[f.Capability], i)
	}
	var keep []int
	for _, indexes := range byCapability {
		keep = append(keep, best(all, indexes, perCapability)...)
	}
	keep = best(all, keep, total)
	slices.Sort(keep)

	kept = make([]match.Finding, 0, len(keep))
	for _, i := range keep {
		kept = append(kept, all[i])
	}
	return kept, len(all) - len(kept)
}

// toStored turns the findings that are kept into what is stored. A sentence the database cannot hold is
// cleaned of what it cannot hold, and one that has nothing left is not stored.
func toStored(kept []match.Finding) []docstore.Finding {
	out := make([]docstore.Finding, 0, len(kept))
	for _, f := range kept {
		snippet := storable(f.Snippet)
		if snippet == "" {
			continue
		}
		out = append(out, docstore.Finding{
			Document: f.Document, Capability: string(f.Capability), Class: string(f.Class),
			Page: f.Page, Paragraph: f.Paragraph, Snippet: snippet,
		})
	}
	return out
}

// fit keeps of the findings those that fit in the encoded list that an upload may store (budget bytes), the ones that
// matter most first and of equal weight the earliest, and says how many it left out. The findings are returned in the
// order they came in. The number of findings is bounded, and each of them is bounded, but not both together: with the
// longest path and the longest sentence, in characters that take many bytes once they are encoded, the most findings an
// upload keeps come to more than the store holds, and the upload would be refused for it, as a failure of the service,
// for an archive of a few kilobytes. It is kept with fewer findings instead, as it is when there are more of them than
// an upload keeps. The first that does not fit ends the list, so a small one of less weight does not take the place
// of a large one that matters more.
func fit(found []docstore.Finding, budget int) (kept []docstore.Finding, dropped int) {
	order := make([]int, len(found))
	for i := range order {
		order[i] = i
	}
	slices.SortStableFunc(order, func(a, b int) int {
		return classRank[match.FindingClass(found[a].Class)] - classRank[match.FindingClass(found[b].Class)]
	})

	left := budget - 1 // the brackets, and the comma that the last of them does not have
	keep := make([]bool, len(found))
	for n, i := range order {
		size, err := docstore.FindingBytes(found[i])
		if err != nil || size > left {
			dropped = len(found) - n
			break
		}
		left -= size
		keep[i] = true
	}
	kept = make([]docstore.Finding, 0, len(found)-dropped)
	for i, f := range found {
		if keep[i] {
			kept = append(kept, f)
		}
	}
	return kept, dropped
}

// best returns at most n of the indexes: those of the findings that matter most, and of equal weight
// the earliest.
func best(all []match.Finding, indexes []int, n int) []int {
	if len(indexes) <= n {
		return indexes
	}
	ranked := slices.Clone(indexes)
	slices.SortStableFunc(ranked, func(a, b int) int {
		if ra, rb := classRank[all[a].Class], classRank[all[b].Class]; ra != rb {
			return ra - rb
		}
		return a - b
	})
	return ranked[:n]
}

// storable makes a snippet text the database can hold: valid UTF-8 without NUL.
func storable(s string) string {
	s = strings.ToValidUTF8(s, "�")
	return strings.TrimSpace(strings.ReplaceAll(s, "\x00", ""))
}
