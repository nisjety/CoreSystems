package config

import (
	"fmt"
	"os"
	"strings"

	"github.com/triodelab/anbud/internal/secret"
)

const defaultSyncServiceName = "doffin-sync"

// Sync is the configuration of the Doffin raw-store sync (cmd/doffin-sync). The
// sync serves no HTTP and holds no service key, so it has no port, no
// ANBUD_API_KEY and no secondary Doffin key. It reads the database and the
// primary Doffin key, and the Doffin settings are checked exactly as the
// service checks them.
type Sync struct {
	// DatabaseURL is ANBUD_DSN: the DSN of the runtime pool, the role the engine
	// writes through.
	DatabaseURL secret.Value
	// MigrateDatabaseURL is the optional ANBUD_MIGRATE_DSN: the DSN of the schema
	// owner. When it is set the migrations run through it, and the runtime pool
	// uses DatabaseURL. When it is unset one DSN does both, as it always did.
	MigrateDatabaseURL secret.Value
	Doffin             Doffin // primary key only; SecondaryAPIKey stays empty
	ServiceName        string
}

// LoadSync reads the sync configuration from the process environment.
func LoadSync() (Sync, error) { return LoadSyncFrom(os.Getenv) }

// LoadSyncFrom reads the configuration of a Doffin sync through get. Every problem
// is reported at once, and secret values are never echoed.
func LoadSyncFrom(get func(string) string) (Sync, error) { return loadSync(get, true) }

// LoadAnonymousSyncFrom reads the configuration of a sync of a source that serves published notices
// anonymously (TED, BOAMP) through get. No Doffin key is required; one that is set is still checked, and
// not used.
func LoadAnonymousSyncFrom(get func(string) string) (Sync, error) { return loadSync(get, false) }

func loadSync(get func(string) string, needDoffinKey bool) (Sync, error) {
	var problems []string

	dsn := trimmed(get, "ANBUD_DSN")
	if dsn == "" {
		problems = append(problems, "ANBUD_DSN is required")
	}

	d, doffinProblems := loadDoffin(get, false)
	problems = append(problems, doffinProblems...)
	if needDoffinKey && !d.Configured() {
		problems = append(problems, "DOFFIN_API_KEY is required")
	}

	if len(problems) > 0 {
		return Sync{}, fmt.Errorf("config: %s", strings.Join(problems, "; "))
	}

	return Sync{
		DatabaseURL:        secret.Value(dsn),
		MigrateDatabaseURL: secret.Value(trimmed(get, EnvMigrateDSN)),
		Doffin:             d,
		ServiceName:        valueOr(get, "SERVICE_NAME", defaultSyncServiceName),
	}, nil
}
