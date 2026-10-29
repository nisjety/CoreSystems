package main

import (
	"context"
	"os"
)

// watchSignals is the signal policy of a run. The first signal cancels the run, so
// it winds down and ends as cancelled with exit 5. A second signal exits at once
// with exit 5, without waiting for the run. It returns
// when sigs is closed, or after it has called exit. exit is os.Exit in the CLI.
func watchSignals(sigs <-chan os.Signal, cancel context.CancelFunc, exit func(int)) {
	if _, ok := <-sigs; !ok {
		return
	}
	cancel()
	if _, ok := <-sigs; !ok {
		return
	}
	exit(exitCancelled)
}
