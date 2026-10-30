package demo

import (
	"strconv"

	"github.com/triodelab/anbud/internal/noticeid"
)

// A notice published on Doffin and on TED is one eForms notice held twice: the raw store keeps each
// source's copy as evidence, under its own id, and both carry the same eForms notice identifier
// (cbc:ID, the UUID; docs/SOURCES.md verified 19 of 22 on 2026-10-09). The readers show it once.

// twin is what decides between two copies of one notice.
type twin struct {
	noticeID string // the eForms notice identifier; empty when the body could not be read
	version  string // cbc:VersionID, such as "01"
	source   string
}

// keepOnePerNotice says, for each item in order, whether a reader shows it. Items with the same
// eForms notice identifier from different sources are one notice held twice: the best copy (the
// higher version, and on a tie the source ranked higher by sourceRank) decides the source, and the
// copies of the other source are dropped. Copies within one source are left as they were:
// versions of one notice there are the amendment tracking's to group (superseded), not this rule's. An
// item whose identifier is empty (a body the parser refused) is always kept.
func keepOnePerNotice(items []twin) []bool {
	keep := make([]bool, len(items))
	best := make(map[string]int, len(items))
	for i, it := range items {
		keep[i] = true
		if it.noticeID == "" {
			continue
		}
		if j, seen := best[it.noticeID]; !seen || preferTwin(it, items[j]) {
			best[it.noticeID] = i
		}
	}
	for i, it := range items {
		if it.noticeID != "" && it.source != items[best[it.noticeID]].source {
			keep[i] = false
		}
	}
	return keep
}

// preferTwin reports whether a is the better copy of the notice b is a copy of.
func preferTwin(a, b twin) bool {
	if va, vb := versionNumber(a.version), versionNumber(b.version); va != vb {
		return va > vb
	}
	return sourceRank(a.source) > sourceRank(b.source)
}

// sourceRank orders the sources of one notice when their versions tie. Doffin's copy keeps the id the app
// already knows; TED's is the copy the EU publishes; BOAMP's repeats a TED notice in France's bulletin.
func sourceRank(source string) int {
	switch source {
	case noticeid.SourceDoffin:
		return 2
	case noticeid.SourceTED:
		return 1
	}
	return 0
}

// versionNumber reads cbc:VersionID ("01", "2"); a missing or unreadable one is the lowest.
func versionNumber(v string) int {
	n, err := strconv.Atoi(v)
	if err != nil || n < 0 {
		return -1
	}
	return n
}
