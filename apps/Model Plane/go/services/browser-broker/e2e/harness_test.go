//go:build e2e

// Package e2e drives a real browser-broker container over gRPC. It is started
// by e2e/run.sh, which builds the broker image from the current tree, a
// throwaway Postgres and a stub identity provider on a private docker network,
// and exports the container names and the key file path below. Run on its own
// it skips.
package e2e

import (
	"context"
	"fmt"
	"net/http"
	"os"
	"os/exec"
	"strings"
	"testing"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/browser-broker/e2e/idp"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
)

// Scope names the broker's authz requires of a service principal.
const (
	scopeRead  = "browser:read"
	scopeWrite = "browser:write"
)

// The hosts every scenario grants. None is ever printed.
var (
	parentHosts = []string{"example.com", "docs.example.org"}
	otherHosts  = []string{"example.net"}
)

// env is the running stack plus a token signer.
type env struct {
	key    *idp.Key
	issuer string
	org    string
	org2   string
	broker string // container names
	pg     string
	pgUser string
	pgDB   string

	conn *grpc.ClientConn
	cli  mpv1.BrowserBrokerClient
	http string // http://127.0.0.1:<port> of the health listener
}

func newEnv(t *testing.T) *env {
	t.Helper()
	get := func(name string) string {
		v := os.Getenv(name)
		if v == "" {
			t.Skipf("%s is not set: run apps/Model Plane/go/services/browser-broker/e2e/run.sh", name)
		}
		return v
	}
	key, err := idp.LoadPrivatePEM(get("E2E_KEY_FILE"), idp.DefaultKID)
	if err != nil {
		t.Fatalf("load the run-time signing key: %v", err)
	}
	suffix := fmt.Sprintf("%d", time.Now().UnixNano()%1_000_000)
	e := &env{
		key:    key,
		issuer: get("E2E_ISSUER"),
		org:    "e2e-org-" + suffix,
		org2:   "e2e-other-org-" + suffix,
		broker: get("E2E_BROKER"),
		pg:     get("E2E_PG"),
		pgUser: get("E2E_PG_USER"),
		pgDB:   get("E2E_PG_DB"),
	}
	e.reconnect(t)
	t.Cleanup(func() {
		if e.conn != nil {
			_ = e.conn.Close()
		}
	})
	return e
}

// ---- docker ----------------------------------------------------------------

func docker(args ...string) (string, error) {
	out, err := exec.Command("docker", args...).CombinedOutput()
	return strings.TrimSpace(string(out)), err
}

// hostPort resolves the published host port of a container port. A random
// published port can change when the container restarts, so it is looked up
// again after every restart.
func hostPort(container, port string) (string, error) {
	out, err := docker("port", container, port)
	if err != nil {
		return "", fmt.Errorf("docker port %s %s: %w", container, port, err)
	}
	line := strings.SplitN(out, "\n", 2)[0]
	i := strings.LastIndex(line, ":")
	if i < 0 {
		return "", fmt.Errorf("unexpected docker port output")
	}
	return line[i+1:], nil
}

// reconnect resolves the broker's published ports and dials gRPC again.
func (e *env) reconnect(t *testing.T) {
	t.Helper()
	if e.conn != nil {
		_ = e.conn.Close()
		e.conn = nil
	}
	grpcPort, err := hostPort(e.broker, "9095/tcp")
	if err != nil {
		t.Fatalf("resolve the broker gRPC port: %v", err)
	}
	httpPort, err := hostPort(e.broker, "8087/tcp")
	if err != nil {
		t.Fatalf("resolve the broker health port: %v", err)
	}
	conn, err := grpc.NewClient("127.0.0.1:"+grpcPort, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		t.Fatalf("dial the broker: %v", err)
	}
	e.conn, e.cli, e.http = conn, mpv1.NewBrowserBrokerClient(conn), "http://127.0.0.1:"+httpPort
}

// httpStatus returns the status code of GET path on the health listener, or 0
// when nothing answers.
func (e *env) httpStatus(path string) int {
	client := &http.Client{Timeout: 5 * time.Second}
	resp, err := client.Get(e.http + path)
	if err != nil {
		return 0
	}
	defer resp.Body.Close()
	return resp.StatusCode
}

// waitReady polls /readyz until it answers 200 (re-resolving the port each
// time: a restart may not have republished it yet).
func (e *env) waitReady(t *testing.T, within time.Duration) bool {
	t.Helper()
	deadline := time.Now().Add(within)
	for time.Now().Before(deadline) {
		if port, err := hostPort(e.broker, "8087/tcp"); err == nil {
			e.http = "http://127.0.0.1:" + port
			if e.httpStatus("/readyz") == 200 {
				return true
			}
		}
		time.Sleep(time.Second)
	}
	return false
}

// restartBroker runs docker restart on the broker and waits until it is ready
// and dialled again.
func (e *env) restartBroker(t *testing.T) {
	t.Helper()
	if out, err := docker("restart", e.broker); err != nil {
		t.Fatalf("docker restart broker: %v %s", err, out)
	}
	if !e.waitReady(t, 90*time.Second) {
		logs, _ := docker("logs", "--tail", "15", e.broker)
		t.Fatalf("the broker did not become ready after the restart; last log lines (the broker logs counts and classes only):\n%s", logs)
	}
	e.reconnect(t)
}

// sql runs one query against the throwaway Postgres and returns its trimmed
// output (unaligned, tuples only).
func (e *env) sql(t *testing.T, query string) string {
	t.Helper()
	out, err := docker("exec", e.pg, "psql", "-X", "-tA", "-U", e.pgUser, "-d", e.pgDB, "-c", query)
	if err != nil {
		t.Fatalf("psql: %v %s", err, out)
	}
	return out
}

func (e *env) count(t *testing.T, where string) int {
	t.Helper()
	q := "SELECT count(*) FROM browser_grants"
	if where != "" {
		q += " WHERE " + where
	}
	var n int
	if _, err := fmt.Sscanf(e.sql(t, q), "%d", &n); err != nil {
		t.Fatalf("read a row count: %v", err)
	}
	return n
}

// ---- tokens ----------------------------------------------------------------

func (e *env) sign(t *testing.T, spec idp.Spec) string {
	t.Helper()
	raw, err := e.key.Sign(spec)
	if err != nil {
		t.Fatalf("sign: %v", err)
	}
	return raw
}

func (e *env) person(t *testing.T, user string) string {
	return e.sign(t, idp.Person(e.issuer, e.org, user))
}

func (e *env) personInOrg2(t *testing.T, user string) string {
	return e.sign(t, idp.Person(e.issuer, e.org2, user))
}

func (e *env) personZDR(t *testing.T, user string) string {
	spec := idp.Person(e.issuer, e.org, user)
	zdr := true
	spec.ZDR = &zdr
	return e.sign(t, spec)
}

func (e *env) service(t *testing.T, id string, scopes ...string) string {
	return e.sign(t, idp.Service(e.issuer, e.org, id, scopes...))
}

// ---- RPC wrappers (token in, typed answer out) -------------------------------

func rpcCtx(token string) (context.Context, context.CancelFunc) {
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	if token != "" {
		ctx = metadata.AppendToOutgoingContext(ctx, "authorization", "Bearer "+token)
	}
	return ctx, cancel
}

func (e *env) acquire(token string, req *mpv1.AcquireGrantRequest) (*mpv1.AcquireGrantResponse, error) {
	ctx, cancel := rpcCtx(token)
	defer cancel()
	return e.cli.AcquireGrant(ctx, req)
}

func (e *env) validate(token, id string) (*mpv1.ValidateGrantResponse, error) {
	ctx, cancel := rpcCtx(token)
	defer cancel()
	return e.cli.ValidateGrant(ctx, &mpv1.ValidateGrantRequest{GrantId: id})
}

func (e *env) revoke(token, id, reason string) (*mpv1.RevokeGrantResponse, error) {
	ctx, cancel := rpcCtx(token)
	defer cancel()
	return e.cli.RevokeGrant(ctx, &mpv1.RevokeGrantRequest{GrantId: id, Reason: reason})
}

func (e *env) derive(token string, req *mpv1.DeriveRunGrantRequest) (*mpv1.DeriveRunGrantResponse, error) {
	ctx, cancel := rpcCtx(token)
	defer cancel()
	return e.cli.DeriveRunGrant(ctx, req)
}

func (e *env) health(token string) (*mpv1.BrowserHealthResponse, error) {
	ctx, cancel := rpcCtx(token)
	defer cancel()
	return e.cli.Health(ctx, &mpv1.BrowserHealthRequest{})
}

// session is the usual non-ZDR session grant request.
func (e *env) session(key string, hosts []string) *mpv1.AcquireGrantRequest {
	return &mpv1.AcquireGrantRequest{SessionKey: key, Mode: "cloud", OrgId: e.org, AllowedDomains: hosts}
}

// runReq is a derive request under parent.
func (e *env) runReq(parent, key string, hosts []string) *mpv1.DeriveRunGrantRequest {
	return &mpv1.DeriveRunGrantRequest{ParentGrantId: parent, SessionKey: key, OrgId: e.org, AllowedDomains: hosts, Purpose: "ai_run"}
}

// ---- assertions ---------------------------------------------------------------

// sc counts assertions of one scenario.
type sc struct {
	t    *testing.T
	name string
	n    int
}

func (s *sc) ok(cond bool, format string, args ...any) {
	s.t.Helper()
	s.n++
	if !cond {
		s.t.Errorf("["+s.name+"] FAIL: "+format, args...)
	}
}

// must is ok that also stops the scenario: later steps depend on it.
func (s *sc) must(cond bool, format string, args ...any) {
	s.t.Helper()
	s.n++
	if !cond {
		s.t.Fatalf("["+s.name+"] FAIL (fatal): "+format, args...)
	}
}

// code asserts the gRPC code of err (codes.OK for nil).
func (s *sc) code(err error, want codes.Code, what string) {
	s.t.Helper()
	s.n++
	if got := status.Code(err); got != want {
		s.t.Errorf("[%s] FAIL: %s: code %s, want %s", s.name, what, got, want)
	}
}

func (s *sc) done() {
	s.t.Logf("SCENARIO %s: %d assertions", s.name, s.n)
	if !s.t.Failed() {
		fmt.Printf("E2E_SCENARIO_OK %s assertions=%d\n", s.name, s.n)
	}
}

func hasPrefix(id, p string) bool { return strings.HasPrefix(id, p) }

func errMessage(err error) string { return status.Convert(err).Message() }
