// Command doffin-sync syncs the public Doffin corpus into the raw store. It is a
// host-side tool: it serves no HTTP, runs on no scheduler and does not start
// telemetry (plan D16).
package main

import (
	"context"
	"fmt"
	"io"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/triodelab/anbud/internal/config"
	"github.com/triodelab/anbud/internal/noticeid"
	"github.com/triodelab/anbud/internal/sources/boamp"
	"github.com/triodelab/anbud/internal/sources/bzp"
	"github.com/triodelab/anbud/internal/sources/doffin"
	"github.com/triodelab/anbud/internal/sources/fts"
	"github.com/triodelab/anbud/internal/sources/ted"
)

func main() {
	os.Exit(cli(os.Args[1:], os.Getenv, os.Stdout, os.Stderr))
}

// cli runs one sync from the command line and returns the exit code. getenv is the
// environment, passed in so an end-to-end test can supply its own. The order
// follows plan section 4: flags, configuration, database, then runSync.
func cli(args []string, getenv func(string) string, stdout, stderr io.Writer) int {
	req, err := parseArgs(args, time.Now())
	if err != nil {
		fatalf(stderr, err.Error())
		return exitUsage
	}

	load := config.LoadSyncFrom
	if req.source != noticeid.SourceDoffin {
		// TED and BOAMP serve their notices anonymously: no Doffin key.
		load = config.LoadAnonymousSyncFrom
	}
	cfg, err := load(getenv)
	if err != nil {
		// config names variables and never echoes a value.
		fatalf(stderr, err.Error())
		return exitUsage
	}
	if err := checkDatabaseTLS(cfg.DatabaseURL.Reveal()); err != nil {
		// The check names ANBUD_DSN and the rule, never the DSN.
		fatalf(stderr, err.Error())
		return exitUsage
	}
	if err := checkMigrateDatabaseTLS(cfg); err != nil {
		// The same rule for the schema owner's DSN, named by its own variable.
		fatalf(stderr, err.Error())
		return exitUsage
	}
	// The client is built before any database write: a key the client refuses
	// stops the run here, with nothing recorded.
	var client upstream
	switch req.source {
	case noticeid.SourceTED:
		// TED_SEARCH_URL and TED_NOTICE_URL point a test at a loopback TED; unset, they are TED's.
		if client, err = ted.New(ted.Config{Countries: req.countries, SearchURL: getenv("TED_SEARCH_URL"), NoticeURL: getenv("TED_NOTICE_URL")}); err != nil {
			fatalf(stderr, "the TED client refused its configuration; check -countries (ISO 3166-1 alpha-3, such as SWE)")
			return exitUsage
		}
	case noticeid.SourceBOAMP:
		// BOAMP_RECORDS_URL points a test at a loopback BOAMP; unset, it is the open-data portal's.
		if client, err = boamp.New(boamp.Config{RecordsURL: getenv("BOAMP_RECORDS_URL")}); err != nil {
			fatalf(stderr, "the BOAMP client refused its configuration; check BOAMP_RECORDS_URL")
			return exitUsage
		}
	case noticeid.SourceBZP:
		// BZP_RECORDS_URL points a test at a loopback board; unset, it is the Polish board's.
		if client, err = bzp.New(bzp.Config{RecordsURL: getenv("BZP_RECORDS_URL")}); err != nil {
			fatalf(stderr, "the BZP client refused its configuration; check BZP_RECORDS_URL")
			return exitUsage
		}
	case noticeid.SourceFTS:
		// FTS_RECORDS_URL points a test at a loopback service; unset, it is Find a Tender's.
		if client, err = fts.New(fts.Config{RecordsURL: getenv("FTS_RECORDS_URL")}); err != nil {
			fatalf(stderr, "the Find a Tender client refused its configuration; check FTS_RECORDS_URL")
			return exitUsage
		}
	default:
		if client, err = newUpstream(cfg.Doffin); err != nil {
			fatalf(stderr, "the Doffin client refused its configuration; check DOFFIN_API_KEY and DOFFIN_BASE_URL")
			return exitUsage
		}
	}

	// The first signal cancels the run; a second exits at once with code 5 (H4).
	sigs := make(chan os.Signal, 2)
	signal.Notify(sigs, os.Interrupt, syscall.SIGTERM)
	ctx, cancel := context.WithCancel(context.Background())
	go watchSignals(sigs, cancel, os.Exit)
	defer func() {
		// After Stop returns no signal is delivered, so closing sigs is safe and
		// ends the watch.
		signal.Stop(sigs)
		close(sigs)
		cancel()
	}()

	// The pool the engine writes through. With ANBUD_MIGRATE_DSN set, the migrations
	// have run through the schema owner's pool, and this pool is the runtime role's.
	// openStore says why on stderr when the start cannot go on; pgx errors can quote
	// the DSN, so none of their text is printed.
	pool, code := openStore(ctx, cfg, stderr)
	if pool == nil {
		return code
	}
	defer pool.Close()

	return runSync(ctx, pool, client, req, stdout, stderr)
}

// newUpstream builds the Doffin client the way cmd/api does, with the same
// settings from the same loader (plan section 3).
func newUpstream(d config.Doffin) (*doffin.Client, error) {
	return doffin.New(doffin.Config{
		BaseURL:     d.BaseURL,
		APIKey:      d.APIKey,
		SearchRPM:   d.SearchRPM,
		DownloadRPM: d.DownloadRPM,
		Timeout:     d.HTTPTimeout,
	})
}

// cancelledOr is exit 5 when a signal ended the run, and code otherwise.
func cancelledOr(ctx context.Context, code int) int {
	if ctx.Err() != nil {
		return exitCancelled
	}
	return code
}

// fatalf prints one fatal line. Messages name variables and rules, never values.
func fatalf(w io.Writer, msg string) {
	_, _ = fmt.Fprintln(w, "doffin-sync: "+msg)
}
