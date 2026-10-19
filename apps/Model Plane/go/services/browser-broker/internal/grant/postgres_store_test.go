package grant

// Unit tests of PgStore that need no database: they use a pool that was
// closed before the first query, which fails every operation locally.

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"log/slog"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
	sdkmetric "go.opentelemetry.io/otel/sdk/metric"
	"go.opentelemetry.io/otel/sdk/metric/metricdata"
)

// The credentials in this URL must never appear in an error, a log line or a
// metric, whatever fails.
const unitTestDSN = "postgres://leaky-user:s3cret-pw@db.invalid.example:5432/leaky_db"

// The grant package counts store errors through the global meter provider,
// and an instrument created before a provider is installed binds to the first
// one only. The test provider is therefore installed once, in init, before any
// test runs, and no test in this package may install another (storeErrorCount
// fails loudly if one does).
var (
	meterReader   = sdkmetric.NewManualReader()
	meterProvider = sdkmetric.NewMeterProvider(sdkmetric.WithReader(meterReader))
)

func init() { otel.SetMeterProvider(meterProvider) }

// storeErrorCount returns browser_broker_grant_store_errors_total for one op,
// and fails if any data point carries an attribute other than op.
func storeErrorCount(t *testing.T, op string) int64 {
	t.Helper()
	if otel.GetMeterProvider() != metric.MeterProvider(meterProvider) {
		t.Fatal("another test replaced the global meter provider; the store error counter is bound to the one installed in init")
	}
	var rm metricdata.ResourceMetrics
	if err := meterReader.Collect(context.Background(), &rm); err != nil {
		t.Fatal(err)
	}
	var total int64
	for _, sm := range rm.ScopeMetrics {
		for _, m := range sm.Metrics {
			if m.Name != "browser_broker_grant_store_errors_total" {
				continue
			}
			sum, ok := m.Data.(metricdata.Sum[int64])
			if !ok {
				t.Fatalf("metric data is %T", m.Data)
			}
			for _, dp := range sum.DataPoints {
				if dp.Attributes.Len() != 1 {
					t.Fatalf("store error counter carries attributes %v, want only op", dp.Attributes.ToSlice())
				}
				if v, _ := dp.Attributes.Value(attribute.Key("op")); v.AsString() == op {
					total += dp.Value
				}
			}
		}
	}
	return total
}

func closedStore(t *testing.T) *PgStore {
	t.Helper()
	pool, err := pgxpool.New(context.Background(), unitTestDSN)
	if err != nil {
		t.Fatal(err)
	}
	pool.Close()
	return NewPgStore(pool)
}

// A database failure is the fixed ErrStoreUnavailable text and nothing else
// (no driver text, no host, no credentials), it is counted under op alone, and
// the log line carries the op and the SQLSTATE only.
func TestPgStore_DatabaseFailureIsOpaqueCountedAndLogged(t *testing.T) {
	var logs bytes.Buffer
	prev := slog.Default()
	slog.SetDefault(slog.New(slog.NewTextHandler(&logs, nil)))
	t.Cleanup(func() { slog.SetDefault(prev) })

	store := closedStore(t)
	before := storeErrorCount(t, "get")
	_, err := store.GetScoped(context.Background(), "g_abcdef", "org-leaky", "user-leaky")
	if !errors.Is(err, ErrStoreUnavailable) {
		t.Fatalf("get: %v, want ErrStoreUnavailable", err)
	}
	if err.Error() != ErrStoreUnavailable.Error() {
		t.Fatalf("error text %q carries more than the fixed message", err.Error())
	}
	if got := storeErrorCount(t, "get") - before; got != 1 {
		t.Fatalf("store error counter moved by %d, want 1", got)
	}
	line := logs.String()
	for _, want := range []string{"browser grant store error", "op=get", "sqlstate="} {
		if !strings.Contains(line, want) {
			t.Fatalf("log %q lacks %q", line, want)
		}
	}
	for _, leak := range []string{"s3cret", "leaky", "invalid.example", "g_abcdef"} {
		if strings.Contains(line, leak) {
			t.Fatalf("log %q leaks %q", line, leak)
		}
	}
	// Only op and sqlstate ride on the line beyond time, level and msg.
	for _, key := range []string{"org", "owner", "grant_id", "id=", "domain", "reason", "error="} {
		if strings.Contains(line, key) {
			t.Fatalf("log %q carries %q", line, key)
		}
	}
}

func TestPgStore_StoreErrorKeepsSQLStateOnlyForRetry(t *testing.T) {
	for state, want := range map[string]bool{"40P01": true, "40001": true, "23505": false, "": false, "57014": false} {
		err := fmt.Errorf("x: %w", &storeError{sqlState: state})
		if got := retryableTx(err); got != want {
			t.Errorf("sqlstate %q retryable = %v, want %v", state, got, want)
		}
		if !errors.Is(err, ErrStoreUnavailable) {
			t.Errorf("sqlstate %q: not ErrStoreUnavailable", state)
		}
		if err.Error() != "x: "+ErrStoreUnavailable.Error() {
			t.Errorf("sqlstate %q: text %q", state, err.Error())
		}
	}
	if retryableTx(ErrStoreUnavailable) || retryableTx(nil) || retryableTx(ErrGrantRevoked) {
		t.Fatal("a plain error is retryable")
	}
	// dbErr classifies a real PgError by its code.
	store := closedStore(t)
	err := store.dbErr(context.Background(), "derive", &pgconn.PgError{Code: "40P01", Message: "deadlock detected, row s3cret"})
	if !retryableTx(err) || strings.Contains(err.Error(), "s3cret") {
		t.Fatalf("deadlock error: retryable=%v text=%q", retryableTx(err), err.Error())
	}
	if retryableTx(store.dbErr(context.Background(), "derive", &pgconn.PgError{Code: "22021"})) {
		t.Fatal("22021 must not be retried")
	}
}

func TestValidTextAndReasonSanitising(t *testing.T) {
	if !validText("", "plain", "emoji \U0001F600", "tab\tok") {
		t.Fatal("ordinary text refused")
	}
	if validText("a\x00b") || validText("ok", "\xff\xfe") {
		t.Fatal("NUL or invalid UTF-8 accepted")
	}
	if got := truncateReason("bad\x00rea\x01son\xff \n"); got != "badreason" {
		t.Fatalf("reason %q", got)
	}
	if got := truncateReason("\x00\x00"); got != "" {
		t.Fatalf("reason %q", got)
	}
	long := strings.Repeat("é", 200)
	if got := truncateReason(long); len(got) > maxRevokeReasonBytes || !strings.HasPrefix(long, got) {
		t.Fatalf("long reason len %d", len(got))
	}
}

// A closed pool answers every write path with ErrStoreUnavailable, never a
// panic and never a grant, and invalid text is refused before the database.
func TestPgStore_ClosedPoolPaths(t *testing.T) {
	ctx := context.Background()
	store := closedStore(t)
	if _, err := store.CreateSession(ctx, CreateSessionParams{OrgID: "o", OwnerID: "u", SessionKey: "k", AllowedDomains: []string{"example.com"}, TTL: 1}); !errors.Is(err, ErrStoreUnavailable) {
		t.Fatalf("create: %v", err)
	}
	if _, err := store.CreateSession(ctx, CreateSessionParams{OrgID: "o", OwnerID: "u", SessionKey: "k\x00", TTL: 1}); !errors.Is(err, ErrInvalidGrantText) {
		t.Fatalf("create with NUL: %v", err)
	}
	if err := store.RevokeScoped(ctx, "g_x", "o", "u", "r\x00"); !errors.Is(err, ErrStoreUnavailable) {
		t.Fatalf("revoke: %v (a NUL reason must not be the failure)", err)
	}
	if _, err := store.PurgeExpired(ctx, store.now()); !errors.Is(err, ErrStoreUnavailable) {
		t.Fatalf("purge: %v", err)
	}
}
