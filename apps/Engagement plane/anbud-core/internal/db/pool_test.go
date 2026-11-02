package db

import (
	"context"
	"strings"
	"testing"
	"time"
)

func TestOpenRejectsAnInvalidDSNWithoutEchoingIt(t *testing.T) {
	t.Parallel()

	const password = "hunter2-do-not-leak"
	_, err := Open(context.Background(), "postgres://user:"+password+"@host:notaport/db")
	if err == nil {
		t.Fatal("expected an error for an invalid DSN")
	}
	if strings.Contains(err.Error(), password) {
		t.Fatalf("error leaked the password: %q", err)
	}
}

func TestOpenReportsAnUnreachableDatabaseWithoutLeakingThePassword(t *testing.T) {
	t.Parallel()

	const password = "pw-do-not-leak"
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()

	// Port 1 on loopback refuses the connection immediately.
	_, err := Open(ctx, "postgres://u:"+password+"@127.0.0.1:1/anbud_core?sslmode=disable&connect_timeout=2")
	if err == nil {
		t.Fatal("expected a connection error")
	}
	if strings.Contains(err.Error(), password) {
		t.Fatalf("error leaked the password: %q", err)
	}
}
