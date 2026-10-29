// Command import-held-notices copies a snapshot of already-held live Doffin evidence
// from NDJSON on stdin into the configured store. It does not call Doffin or mark any day complete.
package main

import (
	"context"
	"encoding/json"
	"fmt"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/triodelab/anbud/internal/doffinsync"
	"github.com/triodelab/anbud/internal/rawstore"
	"github.com/triodelab/anbud/internal/sources/doffin"
	"io"
	"os"
	"time"
)

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, "held-notice import failed (check database configuration and input evidence)")
		os.Exit(1)
	}
}
func run() error {
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Minute)
	defer cancel()
	pool, err := pgxpool.New(ctx, os.Getenv("ANBUD_DSN"))
	if err != nil {
		return err
	}
	defer pool.Close()
	store := rawstore.New(pool)
	today := time.Now().UTC().Truncate(24 * time.Hour).Add(-24 * time.Hour)
	run, err := store.BeginRun(ctx, doffinsync.RunStart{Mode: doffinsync.ModeSync, From: today, To: today})
	if err != nil {
		return err
	}
	counters := doffinsync.Counters{}
	done := false
	defer func() {
		if !done {
			finishCtx, finishCancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer finishCancel()
			_ = store.FinishRun(finishCtx, run, doffinsync.RunOutcome{Reason: doffinsync.ReasonFailed, Counters: counters, LastError: "held-evidence import failed"})
		}
	}()
	decoder := json.NewDecoder(io.LimitReader(os.Stdin, 256<<20))
	for {
		var record json.RawMessage
		err := decoder.Decode(&record)
		if err == io.EOF {
			break
		}
		if err != nil {
			return err
		}
		var envelope struct {
			Kind string
			Page doffinsync.SearchPage
		}
		if err := json.Unmarshal(record, &envelope); err != nil {
			return err
		}
		if envelope.Kind == "search_page" {
			if err := store.PutSearchPage(ctx, run, envelope.Page); err != nil {
				return err
			}
			continue
		}
		var notice doffin.Notice
		if err := json.Unmarshal(record, &notice); err != nil {
			return err
		}
		inserted, err := store.StoreNotice(ctx, run, notice)
		if err != nil {
			return err
		}
		if inserted {
			counters.Stored++
		} else {
			counters.Duplicates++
		}
	}
	if err := store.FinishRun(ctx, run, doffinsync.RunOutcome{Reason: doffinsync.ReasonDone, Counters: counters}); err != nil {
		return err
	}
	done = true
	fmt.Printf("Imported %d held notices (%d already held); no sync days marked complete.\n", counters.Stored, counters.Duplicates)
	return nil
}
