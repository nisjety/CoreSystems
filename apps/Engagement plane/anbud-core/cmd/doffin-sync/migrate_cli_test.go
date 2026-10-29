package main

import (
	"bytes"
	"slices"
	"strings"
	"testing"

	"github.com/triodelab/anbud/internal/db"
)

// cli with ANBUD_MIGRATE_DSN (O2), through the whole command and with the database
// steps stood in for, so none of these needs a database or touches the network.

const migrateCLIKey = "cli-migrate-key-0001"

// withStoreSteps replaces the steps cli takes through openStore for one test. The
// tests that call it do not run in parallel.
func withStoreSteps(t *testing.T, ops storeOps) {
	t.Helper()
	old := storeSteps
	storeSteps = ops
	t.Cleanup(func() { storeSteps = old })
}

func migrateCLIEnv(extra map[string]string) map[string]string {
	env := map[string]string{
		"ANBUD_DSN":       storeRuntimeDSN,
		"DOFFIN_API_KEY":  migrateCLIKey,
		"DOFFIN_BASE_URL": "http://127.0.0.1:9/public/v2",
	}
	for k, v := range extra {
		env[k] = v
	}
	return env
}

func runCLI(env map[string]string) (code int, stdout, stderr string) {
	var out, errOut bytes.Buffer
	code = cli([]string{"-from", "2020-01-01", "-to", "2020-01-02"}, mapEnv(env), &out, &errOut)
	return code, out.String(), errOut.String()
}

// An ANBUD_MIGRATE_DSN that would send the owner's password to a host it cannot
// verify is refused by the rule ANBUD_DSN is held to, with the variable's own name, before
// any database or Doffin step and without echoing a part of the DSN.
func TestCLIRefusesAnUnsafeMigrateDSN(t *testing.T) {
	const password, host, user = "cli-owner-marker-pw", "db.example.com", "anbud_migrator"
	cases := []struct{ name, dsn string }{
		{"a remote host that is not verified", "postgres://" + user + ":" + password + "@" + host + ":5432/anbud?sslmode=prefer"},
		{"a remote host with require", "postgres://" + user + ":" + password + "@" + host + ":5432/anbud?sslmode=require"},
		{"a remote host behind a loopback host", "host=127.0.0.1," + host + " user=" + user + " password=" + password + " dbname=anbud sslmode=disable"},
		{"a DSN that does not parse", "postgres://" + user + ":" + password + "@" + host + ":notaport/anbud?sslmode=require"},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			f := newFakeSteps(t)
			withStoreSteps(t, f.steps())

			code, stdout, stderr := runCLI(migrateCLIEnv(map[string]string{"ANBUD_MIGRATE_DSN": c.dsn}))

			if code != exitUsage {
				t.Fatalf("exit = %d, want %d (usage)", code, exitUsage)
			}
			if stdout != "" {
				t.Errorf("stdout = %q, want nothing: no run should have started", stdout)
			}
			if !strings.HasPrefix(stderr, "doffin-sync: ANBUD_MIGRATE_DSN ") {
				t.Errorf("stderr %q does not name ANBUD_MIGRATE_DSN", stderr)
			}
			for _, leak := range []string{password, host, user, c.dsn, migrateCLIKey} {
				if strings.Contains(stderr, leak) {
					t.Errorf("stderr echoes %q", leak)
				}
			}
			if len(f.log) != 0 {
				t.Errorf("the database steps ran (%v) although the DSN was refused", f.log)
			}
		})
	}
}

// ANBUD_DSN is checked first, as it always was, so a configuration with both
// wrong is told about the runtime DSN.
func TestCLIChecksANBUDDSNBeforeTheMigrateDSN(t *testing.T) {
	f := newFakeSteps(t)
	withStoreSteps(t, f.steps())

	code, _, stderr := runCLI(migrateCLIEnv(map[string]string{
		"ANBUD_DSN":         "postgres://u:cli-first-marker-pw@db.example.com:5432/anbud?sslmode=require",
		"ANBUD_MIGRATE_DSN": "postgres://u:cli-second-marker-pw@db2.example.com:5432/anbud?sslmode=require",
	}))

	if code != exitUsage || !strings.HasPrefix(stderr, "doffin-sync: ANBUD_DSN ") {
		t.Errorf("got exit %d and stderr %q, want a usage refusal that names ANBUD_DSN", code, stderr)
	}
	if len(f.log) != 0 {
		t.Errorf("the database steps ran (%v)", f.log)
	}
}

// Through the whole command: ANBUD_MIGRATE_DSN reaches the migration step and
// ANBUD_DSN the engine's pool, and a role the check refuses ends the command with
// exit 1, the fixed message and nothing else on either stream.
func TestCLIRefusesATooPrivilegedRuntimeRoleWithAFixedMessage(t *testing.T) {
	for name, env := range map[string]map[string]string{
		"one DSN":  migrateCLIEnv(nil),
		"two DSNs": migrateCLIEnv(map[string]string{"ANBUD_MIGRATE_DSN": storeOwnerDSN}),
	} {
		t.Run(name, func(t *testing.T) {
			f := newFakeSteps(t)
			f.roleErr = db.ErrRoleTooPrivileged
			withStoreSteps(t, f.steps())

			code, stdout, stderr := runCLI(env)

			if code != exitUsage {
				t.Fatalf("exit = %d, want %d", code, exitUsage)
			}
			if stdout != "" {
				t.Errorf("stdout = %q, want nothing", stdout)
			}
			if want := "doffin-sync: " + db.ErrRoleTooPrivileged.Error() + "\n"; stderr != want {
				t.Errorf("stderr = %q, want %q", stderr, want)
			}
			for _, leak := range append(storeLeaks, migrateCLIKey) {
				if strings.Contains(stderr, leak) {
					t.Errorf("stderr carries %q", leak)
				}
			}
			want := []string{"open ANBUD_DSN", "migrate", "check"}
			if _, two := env["ANBUD_MIGRATE_DSN"]; two {
				want = []string{"open ANBUD_DSN", "open ANBUD_MIGRATE_DSN", "check owner", "migrate", "check"}
			}
			if !slices.Equal(f.log, want) {
				t.Errorf("steps = %v, want %v", f.log, want)
			}
			if f.checkedDSN != storeRuntimeDSN {
				t.Error("the role check judged a DSN other than ANBUD_DSN")
			}
		})
	}
}
