package main

import (
	"context"
	"errors"
	"reflect"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/triodelab/anbud/internal/config"
	"github.com/triodelab/anbud/internal/db"
	"github.com/triodelab/anbud/internal/secret"
)

func fakeDatabaseSteps(events *[]string) databaseSteps {
	return databaseSteps{
		open: func(context.Context, string) (*pgxpool.Pool, error) {
			*events = append(*events, "open")
			return &pgxpool.Pool{}, nil
		},
		close:   func(*pgxpool.Pool) { *events = append(*events, "close") },
		migrate: func(context.Context, *pgxpool.Pool) error { *events = append(*events, "migrate"); return nil },
		runtimeCheck: func(context.Context, db.RowQuerier, string) (db.RoleVerdict, error) {
			*events = append(*events, "runtime")
			return db.RoleOK, nil
		},
		ownerCheck: func(context.Context, db.RowQuerier, string) (db.RoleVerdict, error) {
			*events = append(*events, "owner")
			return db.RoleOK, nil
		},
	}
}

func TestAPIUsesRuntimeRoleWithoutSchemaWrites(t *testing.T) {
	var events []string
	pool, err := openAPIDatabaseWith(context.Background(), config.Config{DatabaseURL: secret.Value("private-dsn"), SkipMigrations: true}, fakeDatabaseSteps(&events))
	if err != nil || pool == nil {
		t.Fatalf("open: %v", err)
	}
	if !reflect.DeepEqual(events, []string{"open", "runtime"}) {
		t.Fatalf("events = %v", events)
	}
}

func TestAPIDatabaseFailureIsClosedAndRedacted(t *testing.T) {
	for _, failure := range []string{"open", "runtime", "privileged"} {
		t.Run(failure, func(t *testing.T) {
			var events []string
			steps := fakeDatabaseSteps(&events)
			switch failure {
			case "open":
				steps.open = func(context.Context, string) (*pgxpool.Pool, error) { return nil, errors.New("secret-password") }
			case "runtime":
				steps.runtimeCheck = func(context.Context, db.RowQuerier, string) (db.RoleVerdict, error) {
					return db.RoleOK, errors.New("secret-password")
				}
			case "privileged":
				steps.runtimeCheck = func(context.Context, db.RowQuerier, string) (db.RoleVerdict, error) { return db.RoleWarn, nil }
			}
			pool, err := openAPIDatabaseWith(context.Background(), config.Config{DatabaseURL: secret.Value("secret-password"), SkipMigrations: true}, steps)
			if err == nil || pool != nil || strings.Contains(err.Error(), "secret-password") {
				t.Fatalf("unsafe outcome: %v", err)
			}
			if failure != "open" && !reflect.DeepEqual(events, []string{"open", "close"}) {
				t.Fatalf("not closed: %v", events)
			}
		})
	}
}

func TestMigrationOnlyRequiresDedicatedOwnerAndClosesPool(t *testing.T) {
	for _, failure := range []string{"", "missing", "open", "owner", "privileged", "migrate"} {
		t.Run(failure, func(t *testing.T) {
			var events []string
			steps := fakeDatabaseSteps(&events)
			switch failure {
			case "open":
				steps.open = func(context.Context, string) (*pgxpool.Pool, error) { return nil, errors.New("secret-password") }
			case "owner":
				steps.ownerCheck = func(context.Context, db.RowQuerier, string) (db.RoleVerdict, error) {
					return db.RoleOK, errors.New("secret-password")
				}
			case "privileged":
				steps.ownerCheck = func(context.Context, db.RowQuerier, string) (db.RoleVerdict, error) { return db.RoleWarn, nil }
			case "migrate":
				steps.migrate = func(context.Context, *pgxpool.Pool) error { return errors.New("secret-password") }
			}
			get := func(key string) string {
				if failure != "missing" && key == config.EnvMigrateDSN {
					return "secret-password"
				}
				return ""
			}
			err := migrateOnlyWith(context.Background(), get, steps)
			if failure == "" {
				if err != nil || !reflect.DeepEqual(events, []string{"open", "owner", "migrate", "close"}) {
					t.Fatalf("migration: %v, %v", err, events)
				}
				return
			}
			if err == nil || strings.Contains(err.Error(), "secret-password") {
				t.Fatalf("unsafe error: %v", err)
			}
			if failure != "missing" && failure != "open" && (len(events) == 0 || events[len(events)-1] != "close") {
				t.Fatalf("not closed: %v", events)
			}
		})
	}
}

func TestLegacyDevelopmentAPIAppliesMigrations(t *testing.T) {
	var events []string
	_, err := openAPIDatabaseWith(context.Background(), config.Config{DatabaseURL: secret.Value("private")}, fakeDatabaseSteps(&events))
	if err != nil || !reflect.DeepEqual(events, []string{"open", "migrate", "runtime"}) {
		t.Fatalf("events = %v, %v", events, err)
	}
}
