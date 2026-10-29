package main

import (
	"errors"
	"flag"
	"io"
	"strings"
	"time"

	"github.com/triodelab/anbud/internal/doffinsync"
	"github.com/triodelab/anbud/internal/noticeid"
	"github.com/triodelab/anbud/internal/sources/doffin"
)

// dateLayout is the form of every date on the command line and in the report.
const dateLayout = "2006-01-02"

// usageLine is printed for every refusal of the command line.
const usageLine = "usage: doffin-sync -from YYYY-MM-DD [-to YYYY-MM-DD] [-recheck N] [-max-downloads N] [-page-size N] [-census] [-source doffin|ted|boamp|fts|bzp] [-countries SWE,DNK,...]"

// request is one validated invocation.
type request struct {
	window       doffinsync.Window
	mode         doffinsync.Mode
	recheck      int
	maxDownloads int
	pageSize     int      // 0 is the engine's default of 100
	source       string   // noticeid.SourceDoffin, SourceTED, SourceBOAMP, SourceFTS or SourceBZP
	countries    []string // TED's buyer countries; nil is ted.DefaultCountries
}

// parseArgs reads the flags and checks the window before any database or Doffin
// work. Every refusal is a usage error that names the flag or the rule at fault
// and never echoes the value. The flag package quotes rejected values in its own
// errors, so its error text is replaced, not passed on.
func parseArgs(args []string, now time.Time) (request, error) {
	fs := flag.NewFlagSet("doffin-sync", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	from := fs.String("from", "", "first UTC day, YYYY-MM-DD (required)")
	to := fs.String("to", "", "last UTC day, YYYY-MM-DD (default: UTC yesterday)")
	recheck := fs.Int("recheck", 0, "re-search the last N days of the window even when done")
	maxDownloads := fs.Int("max-downloads", 0, "stop after N downloads; 0 is unlimited")
	pageSize := fs.Int("page-size", 0, "search hits per page, 1 to 100; 0 is the default of 100")
	census := fs.Bool("census", false, "search and count only; download nothing")
	source := fs.String("source", noticeid.SourceDoffin, "the source to sync: doffin, ted, boamp, fts or bzp")
	countries := fs.String("countries", "", "TED only: buyer countries, ISO 3166-1 alpha-3, comma-separated (default NOR,SWE,DNK,FIN,ISL)")

	if err := fs.Parse(args); err != nil {
		return request{}, errors.New(usageLine)
	}
	if fs.NArg() > 0 {
		return request{}, usageError("no positional arguments are accepted")
	}
	if *from == "" {
		return request{}, usageError("-from is required, as YYYY-MM-DD")
	}
	start, err := time.Parse(dateLayout, *from)
	if err != nil {
		return request{}, usageError("-from must be a date as YYYY-MM-DD")
	}
	end := utcYesterday(now)
	if *to != "" {
		if end, err = time.Parse(dateLayout, *to); err != nil {
			return request{}, usageError("-to must be a date as YYYY-MM-DD")
		}
	}
	window := doffinsync.Window{From: start, To: end}
	if err := window.Validate(now); err != nil {
		return request{}, usageError("the window must be closed UTC days from 2017-01-02 up to yesterday (UTC), with -from not after -to")
	}
	if *recheck < 0 || *maxDownloads < 0 {
		return request{}, usageError("-recheck and -max-downloads must not be negative")
	}
	if *pageSize < 0 || *pageSize > doffin.MaxHitsPerPage {
		return request{}, usageError("-page-size must be between 1 and 100, or 0 for the default")
	}

	switch *source {
	case noticeid.SourceDoffin, noticeid.SourceTED, noticeid.SourceBOAMP, noticeid.SourceFTS, noticeid.SourceBZP:
	default:
		return request{}, usageError("-source must be doffin, ted, boamp, fts or bzp")
	}
	var buyers []string
	if *countries != "" {
		if *source != noticeid.SourceTED {
			return request{}, usageError("-countries applies to -source ted only")
		}
		buyers = strings.Split(*countries, ",")
	}

	mode := doffinsync.ModeSync
	if *census {
		mode = doffinsync.ModeCensus
	}
	return request{window: window, mode: mode, recheck: *recheck, maxDownloads: *maxDownloads, pageSize: *pageSize, source: *source, countries: buyers}, nil
}

// engineOptions turns a request into the engine's options. progress is called after
// each day is recorded.
// sourceName is the request's source, Doffin when none is set, as before there was a choice.
func (r request) sourceName() string {
	if r.source == "" {
		return noticeid.SourceDoffin
	}
	return r.source
}

func engineOptions(req request, progress func(doffinsync.DayOutcome)) doffinsync.Options {
	return doffinsync.Options{
		Mode:         req.mode,
		Recheck:      req.recheck,
		MaxDownloads: req.maxDownloads,
		PageSize:     req.pageSize,
		Progress:     progress,
		ValidID:      func(id string) bool { return noticeid.ValidFor(req.sourceName(), id) },
	}
}

// usageError is a refusal of the command line. It names the rule, not the value.
func usageError(rule string) error {
	return errors.New("usage: " + rule)
}

// utcYesterday is the latest closed day: the UTC calendar day before now's (D7).
// A zone at or ahead of UTC cannot have a day still open at this instant.
func utcYesterday(now time.Time) time.Time {
	n := now.UTC()
	return time.Date(n.Year(), n.Month(), n.Day()-1, 0, 0, 0, 0, time.UTC)
}
