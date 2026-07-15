package main

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
)

func TestConnectWithRetrySurvivesTransientStartupRefusals(t *testing.T) {
	attempts := 0
	want := &pgx.Conn{}
	connect := func(context.Context, string) (*pgx.Conn, error) {
		attempts++
		if attempts < 3 {
			return nil, errors.New("temporary connection refusal")
		}
		return want, nil
	}

	got, err := connectWithRetry(
		context.Background(),
		"postgres://redacted",
		100*time.Millisecond,
		time.Millisecond,
		connect,
	)
	if err != nil {
		t.Fatalf("transient startup refusal should recover: %v", err)
	}
	if got != want || attempts != 3 {
		t.Fatalf("connection = %p after %d attempts, want %p after 3", got, attempts, want)
	}
}

func TestConnectWithRetryRemainsBoundedAndFailClosed(t *testing.T) {
	started := time.Now()
	_, err := connectWithRetry(
		context.Background(),
		"postgres://redacted",
		20*time.Millisecond,
		time.Millisecond,
		func(context.Context, string) (*pgx.Conn, error) {
			return nil, errors.New("persistent connection failure")
		},
	)
	if err == nil {
		t.Fatal("persistent connection failure was accepted")
	}
	if time.Since(started) > time.Second {
		t.Fatal("bounded retry exceeded its test deadline")
	}
}
