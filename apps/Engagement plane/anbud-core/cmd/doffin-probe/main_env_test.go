package main

import (
	"os"
	"testing"
)

// TestMain keeps the operator's ANBUD_DSN out of every test. With it set, run would
// open the database it names and take the budget lock, and a unit test must not
// reach any database but a throwaway one. A test that wants a database sets
// ANBUD_DSN itself, from dbtest.
func TestMain(m *testing.M) {
	_ = os.Unsetenv(envDSN)
	os.Exit(m.Run())
}
