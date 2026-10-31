package main

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/rs/zerolog"

	"github.com/triodelab/anbud/internal/config"
	"github.com/triodelab/anbud/internal/demo"
	"github.com/triodelab/anbud/internal/docread"
	"github.com/triodelab/anbud/internal/docstore"
	"github.com/triodelab/anbud/internal/rawstore"
	"github.com/triodelab/anbud/internal/secret"
)

// TestMain lets the test binary stand in for the service binary: the service reads PDFs by running
// itself with docread.ChildArg, and a test that reads one runs this binary the same way.
func TestMain(m *testing.M) {
	if code, ok := runAsPDFChild(os.Args); ok {
		// A trace a test can look for: a child of that very test process ran. The child has no
		// environment, and so no TMPDIR of its own, but it is started in its parent's temporary
		// directory, and it names the file after its parent.
		if dir, err := os.Getwd(); err == nil {
			_ = os.WriteFile(childMarker(dir, os.Getppid()), nil, 0o600)
		}
		os.Exit(code)
	}
	code := m.Run()
	_ = os.Remove(childMarker(os.TempDir(), os.Getpid())) // what the children of these tests left
	os.Exit(code)
}

// childMarker is the file a PDF child leaves in dir for the process that started it.
func childMarker(dir string, parent int) string {
	return filepath.Join(dir, fmt.Sprintf("anbud-pdf-child-%d", parent))
}

// heldNotices is a notice store that holds every notice, for the tests that only build a service.
type heldNotices struct{}

func (heldNotices) HasNotice(context.Context, string) (bool, error) { return true, nil }

// noNotices is a store with nothing in it: buildDemo only holds it.
type noNotices struct{}

func (noNotices) RecentNotices(context.Context, int, func(rawstore.Document) error) error { return nil }
func (noNotices) Notice(context.Context, string) (rawstore.Document, error) {
	return rawstore.Document{}, rawstore.ErrNotFound
}

const orgCoreToken = "unit-fixture-0123456789abcdef0123456789abcdef"

func TestBuildDemoWithoutOrgCoreLeavesLookupsOff(t *testing.T) {
	t.Parallel()

	svc, err := buildDemo(config.Config{}, noNotices{}, nil, zerolog.Nop())
	if err != nil || svc == nil {
		t.Fatalf("buildDemo: %v", err)
	}
	if _, err := svc.Company(context.Background(), "974760673"); !errors.Is(err, demo.ErrCompanyNotConfigured) {
		t.Errorf("Company: err = %v, want ErrCompanyNotConfigured", err)
	}
}

func TestBuildDemoWithOrgCoreLooksOrganisationsUpAsTheServicePrincipal(t *testing.T) {
	t.Parallel()

	var mu sync.Mutex
	var gotPath, gotID, gotToken string
	orgCore := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		gotPath, gotID, gotToken = r.URL.Path, r.Header.Get("X-Service-Id"), r.Header.Get("X-Service-Token")
		mu.Unlock()
		fmt.Fprint(w, `{"organisasjonsnummer":"974760673","navn":"REGISTERENHETEN I BRØNNØYSUND","organisasjonsform":{"kode":"ORGL"}}`)
	}))
	t.Cleanup(orgCore.Close)

	cfg := config.Config{OrgCore: config.OrgCore{URL: orgCore.URL, Token: secret.Value(orgCoreToken)}}
	svc, err := buildDemo(cfg, noNotices{}, nil, zerolog.Nop())
	if err != nil {
		t.Fatalf("buildDemo: %v", err)
	}
	got, err := svc.Company(context.Background(), "974760673")
	if err != nil {
		t.Fatalf("Company: %v", err)
	}
	mu.Lock()
	defer mu.Unlock()
	if got.Name != "REGISTERENHETEN I BRØNNØYSUND" || gotPath != "/api/v1/brreg/974760673" || gotID != "anbud-core" || gotToken != orgCoreToken {
		t.Errorf("company %+v, org-core saw path %q principal %q token set %v", got, gotPath, gotID, gotToken != "")
	}
}

func TestBuildDemoRefusesAnOrgCoreConfigurationItCannotUse(t *testing.T) {
	t.Parallel()

	cfg := config.Config{OrgCore: config.OrgCore{URL: "http://user:pw@org-core:8080", Token: secret.Value(orgCoreToken)}}
	if _, err := buildDemo(cfg, noNotices{}, nil, zerolog.Nop()); err == nil {
		t.Errorf("buildDemo accepted a URL with credentials")
	}
}

func TestTenderDocumentsAreOffUnlessSwitchedOn(t *testing.T) {
	t.Parallel()

	svc, err := buildTenderDocs(config.Config{}, docstore.New(nil), heldNotices{}, zerolog.Nop())
	if err != nil || svc != nil {
		t.Errorf("buildTenderDocs = %v, %v; want nothing: the feature keeps a company's documents, so it is opt-in", svc, err)
	}
}

func TestTenderDocumentsBuildFromTheConfiguration(t *testing.T) {
	t.Parallel()

	cfg := config.Config{TenderDocs: config.TenderDocs{Enabled: true, Retention: 24 * time.Hour, MaxArchiveBytes: 7 << 20}}
	svc, err := buildTenderDocs(cfg, docstore.New(nil), heldNotices{}, zerolog.Nop())
	if err != nil || svc == nil {
		t.Fatalf("buildTenderDocs: %v", err)
	}
	if svc.MaxArchiveBytes() != 7<<20 {
		t.Errorf("the archive limit is %d, want the configured 7 MiB", svc.MaxArchiveBytes())
	}
}

// What the configuration allows, the store allows: a retention the config accepts must not be one the
// service then refuses at start-up, or the service would fail on a value it just validated.
func TestTheLongestRetentionTheConfigurationAllowsIsOneTheServiceTakes(t *testing.T) {
	t.Parallel()

	cfg, err := config.LoadFrom(func(key string) string {
		return map[string]string{
			"ANBUD_API_KEY": "internal-api-key-value", "ANBUD_DSN": "postgres://u:p@h:5432/d",
			"ANBUD_TENDER_DOCS": "true", "ANBUD_TENDER_DOCS_RETENTION": "17520h",
		}[key]
	})
	if err != nil {
		t.Fatalf("LoadFrom: %v", err)
	}
	if cfg.TenderDocs.Retention != docstore.MaxRetention {
		t.Errorf("the configuration's longest retention is %v, the store's is %v", cfg.TenderDocs.Retention, docstore.MaxRetention)
	}
	if _, err := buildTenderDocs(cfg, docstore.New(nil), heldNotices{}, zerolog.Nop()); err != nil {
		t.Errorf("buildTenderDocs: %v", err)
	}
}

func TestBuildTenderDocsNamesTheVariableWhenTheStoreRefusesIt(t *testing.T) {
	t.Parallel()

	cfg := config.Config{TenderDocs: config.TenderDocs{Enabled: true, Retention: docstore.MaxRetention + time.Hour, MaxArchiveBytes: 1 << 20}}
	_, err := buildTenderDocs(cfg, docstore.New(nil), heldNotices{}, zerolog.Nop())
	if err == nil || !strings.Contains(err.Error(), "ANBUD_TENDER_DOCS") {
		t.Errorf("err = %v, want one that names the variables to check", err)
	}
}

// The service binary is also the reader of PDFs: started with docread.ChildArg first, it is the child
// and not the service. Anything else starts the service.
func TestRunAsPDFChildIsDecidedByTheFirstArgumentAlone(t *testing.T) {
	t.Parallel()

	for _, args := range [][]string{nil, {"anbud-api"}, {"anbud-api", "--other"}, {"anbud-api", "x", docread.ChildArg}, {docread.ChildArg}} {
		if _, ok := runAsPDFChild(args); ok {
			t.Errorf("args %v started the PDF reader", args)
		}
	}
}
