package main

import (
	"context"
	"testing"

	"github.com/jackc/pgx/v5/pgxpool"
)

// contractSwitchGetenv answers getenv for the one switch these tests set: key reads value, and every other
// name reads nothing. It is defined in this file, so the file compiles without any other test helper.
func contractSwitchGetenv(key, value string) func(string) string {
	return func(name string) string {
		if name == key {
			return value
		}
		return ""
	}
}

// TestContractAuditSinkIsBuiltOnlyByTheLiteralTrue pins the switch: only the literal true builds the sink.
// Off, there is no sink and no error, even without a database.
func TestContractAuditSinkIsBuiltOnlyByTheLiteralTrue(t *testing.T) {
	for _, raw := range []string{"", "false", "0", "1", "yes", "TRUE", " true "} {
		sink, err := contractAuditorFromEnv(contractSwitchGetenv("SCHEDULE_CONTRACT_ENABLED", raw), nil)
		if err != nil || sink != nil {
			t.Fatalf("SCHEDULE_CONTRACT_ENABLED=%q: sink=%v err=%v; want neither a sink nor an error", raw, sink, err)
		}
	}
}

// TestContractAuditSinkRefusesToStartWithoutADatabase pins the refusal: with the switch on and no registry
// pool, the sink cannot be built, so main refuses to start.
func TestContractAuditSinkRefusesToStartWithoutADatabase(t *testing.T) {
	sink, err := contractAuditorFromEnv(contractSwitchGetenv("SCHEDULE_CONTRACT_ENABLED", "true"), nil)
	if err == nil || sink != nil {
		t.Fatalf("sink=%v err=%v; want no sink and an error while the switch is true and there is no pool", sink, err)
	}
}

// TestContractAuditSinkIsBuiltOverThePoolWhileOn pins the on path: with the switch true and a pool, the sink
// is built and no error is returned. The pool is created lazily, so this opens no connection.
func TestContractAuditSinkIsBuiltOverThePoolWhileOn(t *testing.T) {
	pool, err := pgxpool.New(context.Background(), "postgres://capability@127.0.0.1:1/registry?sslmode=disable")
	if err != nil {
		t.Fatal(err)
	}
	defer pool.Close()
	sink, err := contractAuditorFromEnv(contractSwitchGetenv("SCHEDULE_CONTRACT_ENABLED", "true"), pool)
	if err != nil || sink == nil {
		t.Fatalf("sink=%v err=%v; want a sink over the pool", sink, err)
	}
}
