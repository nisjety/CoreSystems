package testfixture

import (
	"context"
	"fmt"
	"net/url"
	"regexp"
	"strings"

	"github.com/jackc/pgx/v5/pgxpool"
)

var lifecycleFixtureIDPattern = regexp.MustCompile(`^[0-9a-f]{48}$`)

func ValidateLifecycleTarget(dsn, expectedDatabase, fixtureID string) error {
	parsed, err := url.Parse(dsn)
	if err != nil || (parsed.Scheme != "postgres" && parsed.Scheme != "postgresql") {
		return fmt.Errorf("lifecycle target must be a Postgres URL")
	}
	if host := strings.ToLower(parsed.Hostname()); host != "127.0.0.1" && host != "localhost" {
		return fmt.Errorf("lifecycle target must use a loopback host")
	}
	if strings.TrimPrefix(parsed.EscapedPath(), "/") != expectedDatabase {
		return fmt.Errorf("lifecycle target database must be %q", expectedDatabase)
	}
	if !lifecycleFixtureIDPattern.MatchString(fixtureID) {
		return fmt.Errorf("CONTROL_LIFECYCLE_FIXTURE_ID is invalid")
	}
	return nil
}

func VerifyLifecycleMarker(
	ctx context.Context,
	pool *pgxpool.Pool,
	dsn, expectedDatabase, fixtureID string,
) error {
	if err := ValidateLifecycleTarget(dsn, expectedDatabase, fixtureID); err != nil {
		return err
	}
	var stored string
	if err := pool.QueryRow(ctx, `
SELECT fixture_id
FROM control_lifecycle_fixture
WHERE fixture_id = $1
  AND database_name = $2
  AND current_database() = $2`, fixtureID, expectedDatabase).Scan(&stored); err != nil {
		return fmt.Errorf("runner-created lifecycle fixture marker is missing: %w", err)
	}
	if stored != fixtureID {
		return fmt.Errorf("runner-created lifecycle fixture marker does not match")
	}
	return nil
}
