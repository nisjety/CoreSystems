// Package noticeid names a stored notice across its sources. The raw store keeps each notice under
// its source and the source's own identifier (raw_documents.source, raw_documents.doffin_id): a
// Doffin id such as 2026-116564, a TED publication number such as 741144-2026, or a BOAMP identifier
// such as 26-104530. The shapes overlap (1234-2026 is both a TED number and, with a two-digit year, a
// BOAMP one), so an identifier alone cannot say its source. The public id, the one the API and the app
// see, can: a Doffin id as it is, a TED publication number with the prefix "TED-", and a BOAMP
// identifier with the prefix "BOAMP-". Doffin ids keep their old form, so nothing the app already holds
// changes.
package noticeid

import (
	"regexp"
	"strings"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

// The sources the raw store knows (migrations 0008, 0011, 0012 and 0013).
const (
	SourceDoffin = "doffin"
	SourceTED    = "ted"
	SourceBOAMP  = "boamp"
	SourceFTS    = "fts"
	SourceBZP    = "bzp"
)

// TEDPrefix marks a TED publication number in a public id.
const TEDPrefix = "TED-"

// BOAMPPrefix marks a BOAMP identifier in a public id.
const BOAMPPrefix = "BOAMP-"

// FTSPrefix marks a Find a Tender notice number in a public id.
const FTSPrefix = "FTS-"

// BZPPrefix marks a Polish BZP notice in a public id.
const BZPPrefix = "BZP-"

var (
	tedPattern   = regexp.MustCompile(`^[0-9]{1,8}-[0-9]{4}$`)
	boampPattern = regexp.MustCompile(`^[0-9]{2}-[0-9]{4,9}$`)
	bzpPattern   = regexp.MustCompile(`^[0-9]{4}-[0-9]{8}-[0-9]{2}(-P)?$`)
)

// IsBZPID reports whether id has the shape of a BZP notice: the year, the notice's number in its year, and
// its version, with dashes (2026-00505976-01 is the notice 2026/BZP 00505976/01 at version 01). A planning
// notice's number ends in "/P", which the id carries as "-P". The notice number in BZP's own form carries
// spaces and slashes, which a public id cannot, so the id is that form with dashes in their place.
func IsBZPID(id string) bool { return bzpPattern.MatchString(id) }

// IsTEDID reports whether id has the shape of a TED publication number: the number in its year, a
// dash, the year (741144-2026).
func IsTEDID(id string) bool { return tedPattern.MatchString(id) }

// IsBOAMPID reports whether id has the shape of a BOAMP identifier: the two digits of the year, a
// dash, the number of the notice in that year (26-104530).
func IsBOAMPID(id string) bool { return boampPattern.MatchString(id) }

// IsFTSID reports whether id has the shape of a Find a Tender notice number: the number in its year, a
// dash, the year (101800-2026). It has the shape of a TED publication number, which is why the public id
// carries the prefix.
func IsFTSID(id string) bool { return tedPattern.MatchString(id) }

// ValidFor reports whether id is a well-formed identifier of source.
func ValidFor(source, id string) bool {
	switch source {
	case SourceDoffin:
		return doffin.IsNoticeID(id)
	case SourceTED:
		return IsTEDID(id)
	case SourceBOAMP:
		return IsBOAMPID(id)
	case SourceFTS:
		return IsFTSID(id)
	case SourceBZP:
		return IsBZPID(id)
	}
	return false
}

// Public is the public id of a source's identifier.
func Public(source, id string) string {
	switch source {
	case SourceTED:
		return TEDPrefix + id
	case SourceBOAMP:
		return BOAMPPrefix + id
	case SourceFTS:
		return FTSPrefix + id
	case SourceBZP:
		return BZPPrefix + id
	}
	return id
}

// Parse splits a public id into its source and the source's identifier, and reports whether it is
// well formed.
func Parse(public string) (source, id string, ok bool) {
	if rest, found := strings.CutPrefix(public, TEDPrefix); found {
		return SourceTED, rest, IsTEDID(rest)
	}
	if rest, found := strings.CutPrefix(public, BOAMPPrefix); found {
		return SourceBOAMP, rest, IsBOAMPID(rest)
	}
	if rest, found := strings.CutPrefix(public, FTSPrefix); found {
		return SourceFTS, rest, IsFTSID(rest)
	}
	if rest, found := strings.CutPrefix(public, BZPPrefix); found {
		return SourceBZP, rest, IsBZPID(rest)
	}
	return SourceDoffin, public, doffin.IsNoticeID(public)
}

// Valid reports whether public is a well-formed public id of any source.
func Valid(public string) bool {
	_, _, ok := Parse(public)
	return ok
}
