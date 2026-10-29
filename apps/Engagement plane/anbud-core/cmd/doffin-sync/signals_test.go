package main

import (
	"os"
	"syscall"
	"testing"
	"time"
)

// H4: the first signal cancels the run and the second exits at once with code 5.
// The policy is pure over a signal channel and an exit function, so no process
// is signalled. A closed channel ends the watch, so a test never hangs on it.

func TestOneSignalCancelsWithoutExiting(t *testing.T) {
	sigs := make(chan os.Signal, 2)
	sigs <- os.Interrupt
	close(sigs)

	cancels := 0
	var exits []int
	watchSignals(sigs, func() { cancels++ }, func(code int) { exits = append(exits, code) })

	if cancels != 1 {
		t.Errorf("cancel called %d times, want 1", cancels)
	}
	if len(exits) != 0 {
		t.Errorf("exit called with %v after one signal, want no exit", exits)
	}
}

func TestSecondSignalExitsFiveAtOnce(t *testing.T) {
	sigs := make(chan os.Signal, 2)
	sigs <- os.Interrupt
	sigs <- syscall.SIGTERM
	// The channel stays open: a watch that waited for a third signal would block
	// here, and the guard below would fail the test rather than hang it.

	cancels := 0
	var exits []int
	done := make(chan struct{})
	go func() {
		defer close(done)
		watchSignals(sigs, func() { cancels++ }, func(code int) { exits = append(exits, code) })
	}()

	select {
	case <-done:
	case <-time.After(5 * time.Second):
		t.Fatal("watch did not return after the second signal")
	}
	if cancels != 1 {
		t.Errorf("cancel called %d times, want 1", cancels)
	}
	if len(exits) != 1 || exits[0] != exitCancelled {
		t.Errorf("exit calls = %v, want exactly one call with %d", exits, exitCancelled)
	}
}

func TestNoSignalsNoAction(t *testing.T) {
	sigs := make(chan os.Signal)
	close(sigs)

	watchSignals(sigs, func() { t.Error("cancel called with no signal") }, func(int) { t.Error("exit called with no signal") })
}
