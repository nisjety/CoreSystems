// Command anbud-retention applies a retention period to the stored notice bodies (open owner
// decision O1, docs/PERSONAL-DATA.md section 4a). It is a host-side DBA tool, like doffin-sync: it
// serves no HTTP and runs on no scheduler.
//
// It reads every live body, decides with internal/retention which have been kept the given number
// of months past their last use, and prints them. Only with -apply does it erase them, one by one,
// through anbud_redact_raw_document(), the same audited function as the manual procedure, so each
// erasure leaves a tombstone with reason controller_decision and a request reference that names the
// period and the day ("retention:12m:2026-10-28"). It runs as a person's login in anbud_dba and
// refuses any other role. It prints ids, dates and counts, never a body.
package main

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"time"

	"github.com/triodelab/anbud/internal/retention"
)

// Exit codes. Nothing is erased on 1.
const (
	exitDone    = 0 // the run finished: every expired body was listed, or erased with -apply
	exitUsage   = 1 // usage, configuration or a refused login; nothing was erased
	exitPartial = 2 // -apply reached -limit and expired bodies remain: run again
	exitFailed  = 6 // a database failure ended the run; what was erased before it stays erased
)

const (
	dateLayout   = "2006-01-02"
	envDSN       = "ANBUD_DBA_DSN"
	defaultLimit = 1000
	maxLimit     = 100000
)

type request struct {
	months int
	apply  bool
	asOf   time.Time
	limit  int
}

func main() {
	os.Exit(cli(os.Args[1:], os.Getenv, os.Stdout, os.Stderr))
}

func parseArgs(args []string, now time.Time) (request, error) {
	fs := flag.NewFlagSet("anbud-retention", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	months := fs.Int("months", 0, "")
	apply := fs.Bool("apply", false, "")
	asOf := fs.String("as-of", "", "")
	limit := fs.Int("limit", defaultLimit, "")
	if err := fs.Parse(args); err != nil {
		return request{}, errors.New(usageLine)
	}
	if fs.NArg() > 0 {
		return request{}, errors.New(usageLine)
	}
	if *months == 0 {
		return request{}, errors.New("-months is required: the retention period is a decision, so it has no default")
	}
	if *months < 1 || *months > retention.MaxMonths {
		return request{}, retention.ErrMonths
	}
	r := request{months: *months, apply: *apply, limit: *limit, asOf: now.UTC().Truncate(24 * time.Hour)}
	if *asOf != "" {
		t, err := time.Parse(dateLayout, *asOf)
		if err != nil {
			return request{}, errors.New("-as-of must be a date, YYYY-MM-DD")
		}
		if t.After(r.asOf) {
			return request{}, errors.New("-as-of cannot be later than today")
		}
		r.asOf = t
	}
	if r.limit < 1 || r.limit > maxLimit {
		return request{}, fmt.Errorf("-limit must be between 1 and %d", maxLimit)
	}
	return r, nil
}

const usageLine = "usage: anbud-retention -months N [-apply] [-as-of YYYY-MM-DD] [-limit N]"

// cli runs once and returns the exit code. getenv is passed in so a test can supply its own.
func cli(args []string, getenv func(string) string, stdout, stderr io.Writer) int {
	req, err := parseArgs(args, time.Now())
	if err != nil {
		fatalf(stderr, err.Error())
		return exitUsage
	}
	dsn := getenv(envDSN)
	if dsn == "" {
		fatalf(stderr, envDSN+" is required: the DSN of your own login, a member of anbud_dba")
		return exitUsage
	}
	if err := checkTLS(dsn); err != nil {
		fatalf(stderr, err.Error())
		return exitUsage
	}

	ctx := context.Background()
	pool, err := openPool(ctx, dsn)
	if err != nil {
		fatalf(stderr, "could not open the database; check "+envDSN)
		return exitUsage
	}
	defer pool.Close()

	if err := checkLogin(ctx, pool); err != nil {
		fatalf(stderr, err.Error())
		return exitUsage
	}
	return run(ctx, pool, req, stdout, stderr)
}

// fatalf prints one line. Messages name variables and rules, never values.
func fatalf(w io.Writer, msg string) {
	_, _ = fmt.Fprintln(w, "anbud-retention: "+msg)
}
