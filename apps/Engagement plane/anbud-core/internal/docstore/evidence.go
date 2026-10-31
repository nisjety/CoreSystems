package docstore

import (
	"context"
	"crypto/sha256"
	"encoding/binary"
	"encoding/json"
	"fmt"
	"math"
	"slices"

	"github.com/jackc/pgx/v5"
)

// What the evidence of an organisation's uploads holds on to. A scan of the demo asks what the uploads of an
// organisation say about the notices it looks at, and an organisation may hold two hundred uploads, each with up to a
// megabyte of findings and as much of manifest. Reading them all into memory, as this did until 2026-10-13, came to
// about 370 MB read and 200 MB held for uploads that were built to be that large, which a few kilobytes of archive can
// do. The evidence is cut instead, the way an upload is: the best of what is there is kept, and what is left out is
// counted, so that nothing is lost without a number.
const (
	// MaxEvidenceBytes is how much of the findings that one call to EvidenceFor holds on to, counting the text of each
	// and what a finding takes besides (findingOverhead). The first requirement of every capability of a notice is
	// held whatever is left of it, so that what the gate needs to know, that a requirement is there and where, is
	// never cut: that is at most a few findings for each notice. Real findings are about half a kilobyte, so the
	// budget holds some sixteen thousand of them, which is more than the notices an organisation uploads for.
	MaxEvidenceBytes = 8 << 20
	// MaxEvidencePerNotice is the most findings the evidence of one notice holds: as many as one upload keeps.
	MaxEvidencePerNotice = MaxFindings
	// MaxEvidencePerCapability is the most it holds of one capability: as many as one upload keeps. A large package
	// says "ESPD" on every page of its procedure, and those must not push the one sentence that asks for a
	// certificate out of the list.
	MaxEvidencePerCapability = 50

	// findingOverhead is what a finding in memory takes besides the bytes of its text: the string headers and the
	// numbers, and the room the allocator rounds each string up to.
	findingOverhead = 128
	// evidenceCutAbove is how many findings of a notice are held before they are cut to what a notice keeps, so that a
	// notice with a great many uploads never holds more than that and an upload's worth.
	evidenceCutAbove = 2 * MaxEvidencePerNotice
	classRequirement = "requirement"
)

// findingRank orders what matters for the gate: what asks for a capability, then what says it is not asked for, then
// what only rewards it, then what only names it. It is the order tenderdocs keeps an upload's findings in.
var findingRank = map[string]int{classRequirement: 0, "negated": 1, "award": 2, "mention": 3}

func rankOf(class string) int {
	if r, ok := findingRank[class]; ok {
		return r
	}
	return len(findingRank)
}

// Evidence is what the live uploads of one notice say, together.
type Evidence struct {
	Documents int // distinct files that were read
	Unread    int // distinct files that were in an archive and could not be read
	// Findings are the best of the distinct findings of the uploads, each once, the oldest upload first. All of them,
	// when there are no more than the evidence holds on to (MaxEvidencePerNotice, MaxEvidencePerCapability and
	// MaxEvidenceBytes); otherwise the requirements first, and of equal weight the earliest, with the first requirement of
	// every capability among them always.
	Findings []Finding
	// FindingsDropped is how many distinct findings are not in Findings for that reason: len(Findings) and this are all
	// of them.
	FindingsDropped int
}

// findingBytes is what a finding in memory takes, as far as the budget of the evidence counts.
func findingBytes(f Finding) int {
	return len(f.Document) + len(f.Capability) + len(f.Class) + len(f.Snippet) + findingOverhead
}

// selectEvidence keeps the best of the findings, which are in the order they came in: the first requirement of every
// capability, which is kept whatever else is, and then, best first (a requirement before a negation, a negation before
// an award, an award before a mention, and of equal weight the earliest), as many as fit in perCapability of one
// capability, in perNotice in all and in bytesLeft bytes, the first that does not fit in the bytes ending the list. They
// are returned in the order they came in, with how many were left out.
//
// Applied again to what it returned and what came after, it gives what it would have given once to everything, as long as
// the bytes are not what binds: a finding is left out only when enough that are better are there, and they stay. So a
// notice with a great many findings is cut as it grows, with no bytes bound, and the bytes are applied once, at the end.
func selectEvidence(all []Finding, perCapability, perNotice, bytesLeft int) (kept []Finding, dropped int) {
	if len(all) == 0 {
		return all, 0
	}
	keep := make([]bool, len(all))
	perCap := map[string]int{}
	total, used := 0, 0
	take := func(i int) {
		keep[i] = true
		perCap[all[i].Capability]++
		total++
		used += findingBytes(all[i])
	}

	// What the gate must always be able to tell: that a requirement is there, and where.
	floor := map[string]bool{}
	for i, f := range all {
		if f.Class == classRequirement && !floor[f.Capability] {
			floor[f.Capability] = true
			take(i)
		}
	}

	rest := make([]int, 0, len(all)-total)
	for i := range all {
		if !keep[i] {
			rest = append(rest, i)
		}
	}
	slices.SortStableFunc(rest, func(a, b int) int { return rankOf(all[a].Class) - rankOf(all[b].Class) })
fill:
	for _, i := range rest {
		switch f := all[i]; {
		case total >= perNotice:
			break fill
		case perCap[f.Capability] >= perCapability:
			continue // that capability is full; the next may not be
		case used+findingBytes(f) > bytesLeft:
			break fill // the first that does not fit ends the list
		}
		take(i)
	}

	kept = make([]Finding, 0, total)
	for i, f := range all {
		if keep[i] {
			kept = append(kept, f)
		}
	}
	return kept, len(all) - len(kept)
}

// evidenceBuilder sums the uploads of the notices into what they say together, and holds on to little of it: the
// documents and the findings are counted by a hash of each and not held, and of the findings only the best are. The
// uploads of one notice are added together, the oldest first, and the notices one after another.
type evidenceBuilder struct {
	bytesLeft int // of what the call may hold on to, for the notices that are still to come
	cur       *noticeTally
	out       map[string]Evidence
}

// noticeTally is what the uploads of the notice that is being added say so far.
type noticeTally struct {
	id           string
	read, unread map[[32]byte]struct{} // the files, by a hash of what names each
	seen         map[[32]byte]struct{} // the findings, by a hash of each: counted once, and none held on to for it
	kept         []Finding             // the best of them so far, in the order they came in
	dropped      int                   // the findings left out so far
	buf          []byte                // for hashing
}

func newEvidenceBuilder(bytes int) *evidenceBuilder {
	return &evidenceBuilder{bytesLeft: bytes, out: map[string]Evidence{}}
}

// keyOf is the key of a file in the sets of files: a hash of its hash, or of its path when it has none.
func keyOf(s string) [32]byte { return sha256.Sum256([]byte(s)) }

// hashFinding is a hash of every part of a finding, each with its length, so that a finding is the same as another
// only when every part is, and not when a part of one ends where the next of another begins.
func hashFinding(buf []byte, f Finding) ([32]byte, []byte) {
	buf = buf[:0]
	for _, s := range [...]string{f.Document, f.Capability, f.Class, f.Snippet} {
		buf = binary.AppendUvarint(buf, uint64(len(s)))
		buf = append(buf, s...)
	}
	buf = binary.AppendVarint(buf, int64(f.Page))
	buf = binary.AppendVarint(buf, int64(f.Paragraph))
	return sha256.Sum256(buf), buf
}

// add sums one upload of a notice into what the notice says.
func (b *evidenceBuilder) add(notice string, docs []Document, findings []Finding) {
	if b.cur == nil || b.cur.id != notice {
		b.finish()
		b.cur = &noticeTally{id: notice, read: map[[32]byte]struct{}{}, unread: map[[32]byte]struct{}{}, seen: map[[32]byte]struct{}{}}
	}
	t := b.cur
	for _, d := range docs {
		switch {
		case d.Status == StatusRead:
			t.read[keyOf(d.SHA256)] = struct{}{}
		case unreadStatus[d.Status]:
			t.unread[keyOf(unreadKey(d))] = struct{}{}
		}
	}
	for _, f := range findings {
		var key [32]byte
		key, t.buf = hashFinding(t.buf, f)
		if _, again := t.seen[key]; again {
			continue
		}
		t.seen[key] = struct{}{}
		t.kept = append(t.kept, f)
		if len(t.kept) > evidenceCutAbove {
			t.cut(math.MaxInt)
		}
	}
}

// cut keeps the best of what the notice holds, in bytesLeft bytes.
func (t *noticeTally) cut(bytesLeft int) {
	kept, dropped := selectEvidence(t.kept, MaxEvidencePerCapability, MaxEvidencePerNotice, bytesLeft)
	t.kept, t.dropped = kept, t.dropped+dropped
}

// finish ends the notice that is being added: its findings are cut to what is left of the budget, and what it says is
// kept as it is.
func (b *evidenceBuilder) finish() {
	t := b.cur
	if t == nil {
		return
	}
	t.cut(b.bytesLeft)
	ev := Evidence{Documents: len(t.read), Findings: t.kept, FindingsDropped: t.dropped}
	for key := range t.unread {
		if _, read := t.read[key]; !read { // a file read from one archive is not unread because another could not
			ev.Unread++
		}
	}
	for _, f := range t.kept {
		b.bytesLeft -= findingBytes(f)
	}
	b.bytesLeft = max(b.bytesLeft, 0)
	b.out[t.id] = ev
	b.cur = nil
}

// result is what the notices that were added say.
func (b *evidenceBuilder) result() map[string]Evidence {
	b.finish()
	return b.out
}

const selectEvidenceRows = `
SELECT notice_id, documents, findings
  FROM tenant.tender_uploads
 WHERE org_id = $1 AND notice_id = ANY($2) AND expires_at > now()
 ORDER BY notice_id, created_at, id`

// EvidenceFor sums the live uploads of each of the notices into what they say together. A notice with no upload is
// not in the answer. A file that is in two archives is one file, and a finding that two archives share is one finding.
// The uploads are read one at a time, and only the best of the findings are held on to (Evidence.Findings), with the
// number that were left out (Evidence.FindingsDropped). A request for more than MaxEvidenceNotices notices is refused;
// the caller asks for them in as many calls, each with a budget of its own.
func (s *Store) EvidenceFor(ctx context.Context, org string, noticeIDs []string) (map[string]Evidence, error) {
	if err := checkOrg(org); err != nil {
		return nil, err
	}
	if len(noticeIDs) > MaxEvidenceNotices {
		return nil, invalid("notices")
	}
	for _, id := range noticeIDs {
		if err := checkNotice(id); err != nil {
			return nil, err
		}
	}
	if len(noticeIDs) == 0 {
		return map[string]Evidence{}, nil
	}

	b := newEvidenceBuilder(s.evidenceBytes)
	err := s.scoped(ctx, org, func(tx pgx.Tx) error {
		rows, err := tx.Query(ctx, selectEvidenceRows, org, noticeIDs)
		if err != nil {
			return fmt.Errorf("docstore: read the evidence: %w", err)
		}
		defer rows.Close()
		for rows.Next() {
			var notice string
			var docsJSON, findingsJSON []byte
			if err := rows.Scan(&notice, &docsJSON, &findingsJSON); err != nil {
				return fmt.Errorf("docstore: scan the evidence: %w", err)
			}
			var docs []Document
			var findings []Finding
			if err := json.Unmarshal(docsJSON, &docs); err != nil {
				return fmt.Errorf("docstore: decode a manifest: %w", err)
			}
			if err := json.Unmarshal(findingsJSON, &findings); err != nil {
				return fmt.Errorf("docstore: decode findings: %w", err)
			}
			b.add(notice, docs, findings)
		}
		return rows.Err()
	})
	if err != nil {
		return nil, err
	}
	return b.result(), nil
}
