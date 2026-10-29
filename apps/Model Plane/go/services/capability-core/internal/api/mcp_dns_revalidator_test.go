package api

import (
	"context"
	"errors"
	"net/netip"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

// mcpCandidateRow is one row of the revalidator's `SELECT id, org_id,
// endpoint_url` query.
type mcpCandidateRow struct {
	id, orgID, endpointURL string
}

// fakeMCPCandidateRows is a pgx.Rows fake that yields a fixed set of
// candidate rows — recordingDatabase (defined in mcp_auth_test.go) only holds
// one pgx.Rows per test, so this is what backs its Query stub here.
type fakeMCPCandidateRows struct {
	records []mcpCandidateRow
	index   int
}

func (rows *fakeMCPCandidateRows) Close()                                       {}
func (rows *fakeMCPCandidateRows) Err() error                                   { return nil }
func (rows *fakeMCPCandidateRows) CommandTag() pgconn.CommandTag                { return pgconn.CommandTag{} }
func (rows *fakeMCPCandidateRows) FieldDescriptions() []pgconn.FieldDescription { return nil }
func (rows *fakeMCPCandidateRows) Values() ([]any, error)                       { return nil, nil }
func (rows *fakeMCPCandidateRows) RawValues() [][]byte                          { return nil }
func (rows *fakeMCPCandidateRows) Conn() *pgx.Conn                              { return nil }

func (rows *fakeMCPCandidateRows) Next() bool {
	return rows.index < len(rows.records)
}

func (rows *fakeMCPCandidateRows) Scan(dest ...any) error {
	if len(dest) != 3 {
		return errors.New("fakeMCPCandidateRows: want three scan destinations")
	}
	idPtr, ok := dest[0].(*string)
	if !ok {
		return errors.New("fakeMCPCandidateRows: id destination is not *string")
	}
	orgPtr, ok := dest[1].(*string)
	if !ok {
		return errors.New("fakeMCPCandidateRows: org_id destination is not *string")
	}
	endpointPtr, ok := dest[2].(*string)
	if !ok {
		return errors.New("fakeMCPCandidateRows: endpoint_url destination is not *string")
	}
	record := rows.records[rows.index]
	rows.index++
	*idPtr, *orgPtr, *endpointPtr = record.id, record.orgID, record.endpointURL
	return nil
}

func newMCPCandidateDB(records ...mcpCandidateRow) *recordingDatabase {
	return &recordingDatabase{rows: &fakeMCPCandidateRows{records: records}}
}

func TestMCPDNSRevalidatorSkipsHealthyServer(t *testing.T) {
	db := newMCPCandidateDB(mcpCandidateRow{id: "mcp_1", orgID: "org_1", endpointURL: "https://mcp.example.test/mcp"})
	revalidator := NewMCPDNSRevalidator(db, time.Minute)
	revalidator.resolver = publicMCPResolver{}

	checked, quarantined, err := revalidator.RunOnce(context.Background())
	if err != nil {
		t.Fatalf("RunOnce: %v", err)
	}
	if checked != 1 || quarantined != 0 {
		t.Fatalf("checked=%d quarantined=%d, want 1/0", checked, quarantined)
	}
	if len(db.execs) != 0 {
		t.Fatalf("healthy server must not be written to: %d exec(s)", len(db.execs))
	}
}

func TestMCPDNSRevalidatorQuarantinesDriftedServer(t *testing.T) {
	db := newMCPCandidateDB(mcpCandidateRow{id: "mcp_1", orgID: "org_1", endpointURL: "https://mcp.example.test/mcp"})
	revalidator := NewMCPDNSRevalidator(db, time.Minute)
	// DNS now answers with a CGNAT address -- forbidden today even though
	// the same hostname could have resolved publicly at registration time.
	revalidator.resolver = fixedMCPResolver{addresses: []netip.Addr{netip.MustParseAddr("100.64.0.1")}}

	checked, quarantined, err := revalidator.RunOnce(context.Background())
	if err != nil {
		t.Fatalf("RunOnce: %v", err)
	}
	if checked != 1 || quarantined != 1 {
		t.Fatalf("checked=%d quarantined=%d, want 1/1", checked, quarantined)
	}
	if len(db.execs) != 1 {
		t.Fatalf("want exactly one quarantine write, got %d", len(db.execs))
	}
	call := db.execs[0]
	if !strings.Contains(call.query, "rollout_state='quarantine'") || !strings.Contains(call.query, "enabled=false") {
		t.Fatalf("quarantine exec did not disable/quarantine the row: %q", call.query)
	}
	if len(call.args) != 3 || call.args[1] != "mcp_1" || call.args[2] != "org_1" {
		t.Fatalf("quarantine exec targeted the wrong row: %+v", call.args)
	}
}

func TestMCPDNSRevalidatorSkipsOnTransientResolverError(t *testing.T) {
	db := newMCPCandidateDB(mcpCandidateRow{id: "mcp_1", orgID: "org_1", endpointURL: "https://mcp.example.test/mcp"})
	revalidator := NewMCPDNSRevalidator(db, time.Minute)
	revalidator.resolver = fixedMCPResolver{err: errors.New("temporary dns failure")}

	checked, quarantined, err := revalidator.RunOnce(context.Background())
	if err != nil {
		t.Fatalf("RunOnce: %v", err)
	}
	if checked != 1 || quarantined != 0 {
		t.Fatalf("a resolver error must be skipped, not quarantined: checked=%d quarantined=%d", checked, quarantined)
	}
	if len(db.execs) != 0 {
		t.Fatalf("a transient resolver error must not write to the row: %d exec(s)", len(db.execs))
	}
}

func TestMCPDNSRevalidatorSkipsAllowlistedInternalHost(t *testing.T) {
	t.Setenv("MCP_INTERNAL_ALLOWED_HOSTS", "mcp-bridge")
	db := newMCPCandidateDB(mcpCandidateRow{id: "mcp_1", orgID: "org_1", endpointURL: "http://mcp-bridge:9201"})
	revalidator := NewMCPDNSRevalidator(db, time.Minute)
	// Even if this resolved to a private address, the operator's own
	// allowlist decision at registration time must not be reversed by a
	// background sweep.
	revalidator.resolver = fixedMCPResolver{addresses: []netip.Addr{netip.MustParseAddr("10.0.0.5")}}

	checked, quarantined, err := revalidator.RunOnce(context.Background())
	if err != nil {
		t.Fatalf("RunOnce: %v", err)
	}
	if checked != 1 || quarantined != 0 {
		t.Fatalf("allowlisted internal host must be exempt: checked=%d quarantined=%d", checked, quarantined)
	}
}

func TestMCPDNSRevalidatorQuarantinesEndpointThatNoLongerParses(t *testing.T) {
	// A durable row that predates the current contract (e.g. an embedded
	// credential) must be quarantined without ever reaching the resolver.
	db := newMCPCandidateDB(mcpCandidateRow{id: "mcp_1", orgID: "org_1", endpointURL: "https://user:pass@mcp.example.test/mcp"})
	revalidator := NewMCPDNSRevalidator(db, time.Minute)
	revalidator.resolver = fixedMCPResolver{err: errors.New("must not be called")}

	checked, quarantined, err := revalidator.RunOnce(context.Background())
	if err != nil {
		t.Fatalf("RunOnce: %v", err)
	}
	if checked != 1 || quarantined != 1 {
		t.Fatalf("checked=%d quarantined=%d, want 1/1", checked, quarantined)
	}
}

func TestMCPDNSRevalidatorFailsClosedWithoutResolver(t *testing.T) {
	revalidator := &MCPDNSRevalidator{pool: newMCPCandidateDB()}
	if _, _, err := revalidator.RunOnce(context.Background()); err == nil {
		t.Fatal("RunOnce must fail without a configured resolver rather than silently skip every row")
	}
}

func TestNewMCPDNSRevalidatorDefaultsNonPositiveInterval(t *testing.T) {
	revalidator := NewMCPDNSRevalidator(newMCPCandidateDB(), 0)
	if revalidator.interval != DefaultMCPDNSRevalidationInterval {
		t.Fatalf("interval = %v, want default %v", revalidator.interval, DefaultMCPDNSRevalidationInterval)
	}
}
