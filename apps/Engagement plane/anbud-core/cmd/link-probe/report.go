package main

import (
	"fmt"
	"io"
	"strings"
)

// report holds the counts of one run. It has no field that holds a link, a host or a notice.
type report struct {
	notices, refused, withLinks  int
	distinct, skipped, requested int
	capped                       bool
	families                     map[string]int
	results                      map[string]int
	byFamily                     map[string]map[string]int
	hop                          *hopReport // nil when no second hop was asked for
}

// hopReport counts the second hop: the pages the first hop found, the document links on them, and
// what asking those links gave.
type hopReport struct {
	pages, unreadable, found, otherHost, requested int
	capped                                         bool
	results                                        map[string]int
}

func newReport() report {
	return report{
		families: map[string]int{},
		results:  map[string]int{},
		byFamily: map[string]map[string]int{},
	}
}

func newHop() hopReport {
	return hopReport{results: map[string]int{}}
}

// The order of the lines: fixed, so two runs can be compared by eye.
var (
	familyOrder = []string{"doffin", "login-platform", "norway-other", "other"}
	classOrder  = []string{"document", "login-required", "html-page", "forbidden", "gone", "rate-limited", "server-error", "unreachable", "unclear"}
)

// writeReport prints the counts. Every word in it is a fixed name, a number or a count.
func writeReport(w io.Writer, r report) {
	fmt.Fprintf(w, "notices read: %d, refused: %d, with links: %d\n", r.notices, r.refused, r.withLinks)
	fmt.Fprintf(w, "distinct links: %d\n", r.distinct)
	fmt.Fprintf(w, "by family: %s\n", counts(familyOrder, r.families))
	fmt.Fprintf(w, "not requested (login platform): %d\n", r.skipped)
	fmt.Fprintf(w, "requested: %d, capped at max: %s\n", r.requested, yesNo(r.capped))
	fmt.Fprintf(w, "results: %s\n", counts(classOrder, r.results))
	for _, fam := range familyOrder {
		if len(r.byFamily[fam]) == 0 {
			continue
		}
		fmt.Fprintf(w, "results for %s: %s\n", fam, counts(classOrder, r.byFamily[fam]))
	}
	if h := r.hop; h != nil {
		fmt.Fprintf(w, "second hop: pages read %d, unreadable %d\n", h.pages, h.unreadable)
		fmt.Fprintf(w, "document links on them %d (same host), on other hosts %d (not followed)\n", h.found, h.otherHost)
		fmt.Fprintf(w, "second hop: requested %d, capped at max per page: %s\n", h.requested, yesNo(h.capped))
		fmt.Fprintf(w, "second hop results: %s\n", counts(classOrder, h.results))
	}
}

// counts lists every name in order with its count, so that a zero still shows.
func counts(order []string, m map[string]int) string {
	parts := make([]string, 0, len(order))
	for _, k := range order {
		parts = append(parts, fmt.Sprintf("%s %d", k, m[k]))
	}
	return strings.Join(parts, ", ")
}

func yesNo(b bool) string {
	if b {
		return "yes"
	}
	return "no"
}
