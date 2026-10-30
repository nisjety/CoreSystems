package orgcore

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/triodelab/anbud/internal/secret"
)

// testToken is long enough for org-core's credential rules and starts with none of the prefixes
// org-core refuses. It is not a real credential.
const testToken = "unit-fixture-0123456789abcdef0123456789abcdef"

// sampleEnhet is a response in the shape org-core's GET /api/v1/brreg/:orgnr returns: Enhetsregisteret's
// record, passed through. 974760673 is Brønnøysundregistrene's own, public organisation number;
// the e-mail address and telephone number are invented.
const sampleEnhet = `{
  "organisasjonsnummer": "974760673",
  "navn": "REGISTERENHETEN I BRØNNØYSUND",
  "organisasjonsform": {"kode": "ORGL", "beskrivelse": "Organisasjonsledd"},
  "forretningsadresse": {"adresse": ["Havnegata 48"], "postnummer": "8900", "poststed": "BRØNNØYSUND", "kommune": "BRØNNØY", "kommunenummer": "1813", "land": "Norge", "landkode": "NO"},
  "naeringskode1": {"kode": "84.110", "beskrivelse": "Generell offentlig administrasjon"},
  "antallAnsatte": 550,
  "konkurs": false,
  "underAvvikling": false,
  "underTvangsavviklingEllerTvangsopplosning": false,
  "epostadresse": "invented@example.invalid",
  "telefon": "+47 00 00 00 00"
}`

func newServer(t *testing.T, handler http.HandlerFunc) (*Client, *httptest.Server) {
	t.Helper()
	srv := httptest.NewServer(handler)
	t.Cleanup(srv.Close)
	c, err := New(Config{BaseURL: srv.URL, Token: secret.Value(testToken)})
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	return c, srv
}

func TestLookupSendsTheServiceCredentialAndReadsTheRecord(t *testing.T) {
	t.Parallel()

	c, _ := newServer(t, func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodGet || r.URL.Path != "/api/v1/brreg/974760673" {
			t.Errorf("request = %s %s", r.Method, r.URL.Path)
		}
		if r.Header.Get("X-Service-Id") != "anbud-core" || r.Header.Get("X-Service-Token") != testToken {
			t.Errorf("service headers = %q / token set %v", r.Header.Get("X-Service-Id"), r.Header.Get("X-Service-Token") != "")
		}
		for _, h := range []string{"X-User-Id", "X-Org-Id", "X-User-Role", "X-Delegation-Signature"} {
			if r.Header.Get(h) != "" {
				t.Errorf("header %s sent: a lookup carries no user identity", h)
			}
		}
		fmt.Fprint(w, sampleEnhet)
	})

	org, err := c.Lookup(context.Background(), "NO 974 760 673 MVA")
	if err != nil {
		t.Fatalf("Lookup: %v", err)
	}
	want := Organisation{
		OrgNumber: "974760673", Name: "REGISTERENHETEN I BRØNNØYSUND", OrgForm: "ORGL",
		NACE: "84.110", NACEText: "Generell offentlig administrasjon",
		Municipality: "BRØNNØY", MunicipalityNumber: "1813", Employees: intPtr(550),
	}
	if !equalOrg(org, want) {
		t.Errorf("org = %+v\nwant %+v", org, want)
	}
}

func TestLookupNeverKeepsContactDetails(t *testing.T) {
	t.Parallel()

	c, _ := newServer(t, func(w http.ResponseWriter, _ *http.Request) { fmt.Fprint(w, sampleEnhet) })
	org, err := c.Lookup(context.Background(), "974760673")
	if err != nil {
		t.Fatalf("Lookup: %v", err)
	}
	printed := fmt.Sprintf("%+v", org)
	for _, personal := range []string{"example.invalid", "+47", "Havnegata"} {
		if strings.Contains(printed, personal) {
			t.Errorf("the organisation carries a contact detail or street address")
		}
	}
}

func TestLookupReadsTheStatusFlags(t *testing.T) {
	t.Parallel()

	body := strings.NewReplacer(`"konkurs": false`, `"konkurs": true`, `"underAvvikling": false`, `"underAvvikling": true`).Replace(sampleEnhet)
	c, _ := newServer(t, func(w http.ResponseWriter, _ *http.Request) { fmt.Fprint(w, body) })
	org, err := c.Lookup(context.Background(), "974760673")
	if err != nil {
		t.Fatalf("Lookup: %v", err)
	}
	if !org.Bankrupt || !org.UnderLiquidation || org.ForcedLiquidation {
		t.Errorf("flags = bankrupt %v liquidation %v forced %v", org.Bankrupt, org.UnderLiquidation, org.ForcedLiquidation)
	}
	if org.CanBid() {
		t.Errorf("a bankrupt organisation can bid")
	}
}

func TestCanBidIsFalseForEveryRegisterFlag(t *testing.T) {
	t.Parallel()

	for name, o := range map[string]Organisation{
		"bankrupt":           {Bankrupt: true},
		"being wound up":     {UnderLiquidation: true},
		"forced liquidation": {ForcedLiquidation: true},
	} {
		if o.CanBid() {
			t.Errorf("%s: can bid", name)
		}
	}
	if !(Organisation{}).CanBid() {
		t.Errorf("a company with no flag cannot bid")
	}
}

func TestLookupRefusesABadOrgNumberWithoutACall(t *testing.T) {
	t.Parallel()

	var calls atomic.Int32
	c, _ := newServer(t, func(http.ResponseWriter, *http.Request) { calls.Add(1) })
	for _, bad := range []string{"974760674", "../organizations/x", "97476067", ""} {
		if _, err := c.Lookup(context.Background(), bad); !errors.Is(err, ErrInvalidOrgNumber) {
			t.Errorf("Lookup(%q) err = %v, want ErrInvalidOrgNumber", bad, err)
		}
	}
	if calls.Load() != 0 {
		t.Errorf("%d calls made for invalid numbers", calls.Load())
	}
}

func TestLookupMapsOrgCoreStatuses(t *testing.T) {
	t.Parallel()

	for status, want := range map[int]error{
		http.StatusNotFound:            ErrNotFound,
		http.StatusUnauthorized:        ErrUnauthorized,
		http.StatusForbidden:           ErrScopeDenied,
		http.StatusBadGateway:          ErrUpstream,
		http.StatusServiceUnavailable:  ErrUnavailable,
		http.StatusInternalServerError: ErrUnexpectedStatus,
	} {
		c, _ := newServer(t, func(w http.ResponseWriter, _ *http.Request) {
			w.WriteHeader(status)
			fmt.Fprint(w, `{"error":"`+testToken+`"}`)
		})
		_, err := c.Lookup(context.Background(), "974760673")
		if !errors.Is(err, want) {
			t.Errorf("status %d: err = %v, want %v", status, err, want)
		}
		if err != nil && strings.Contains(err.Error(), testToken) {
			t.Errorf("status %d: the error echoes the response body", status)
		}
	}
}

func TestLookupRefusesARecordForAnotherNumber(t *testing.T) {
	t.Parallel()

	c, _ := newServer(t, func(w http.ResponseWriter, _ *http.Request) {
		fmt.Fprint(w, strings.Replace(sampleEnhet, `"974760673"`, `"923609016"`, 1))
	})
	if _, err := c.Lookup(context.Background(), "974760673"); !errors.Is(err, ErrMismatch) {
		t.Errorf("err = %v, want ErrMismatch", err)
	}
}

func TestLookupRefusesAnOversizedOrMalformedBody(t *testing.T) {
	t.Parallel()

	for name, body := range map[string]string{
		// A valid record followed by padding: only the size cap can refuse it.
		"oversized": sampleEnhet + strings.Repeat(" ", maxBody),
		"malformed": `{"organisasjonsnummer":`,
	} {
		c, _ := newServer(t, func(w http.ResponseWriter, _ *http.Request) { fmt.Fprint(w, body) })
		if _, err := c.Lookup(context.Background(), "974760673"); !errors.Is(err, ErrBadResponse) {
			t.Errorf("%s: err = %v, want ErrBadResponse", name, err)
		}
	}
}

func TestLookupNeverFollowsARedirect(t *testing.T) {
	t.Parallel()

	var leaked atomic.Bool
	elsewhere := httptest.NewServer(http.HandlerFunc(func(_ http.ResponseWriter, r *http.Request) {
		if r.Header.Get("X-Service-Token") != "" {
			leaked.Store(true)
		}
	}))
	t.Cleanup(elsewhere.Close)
	c, _ := newServer(t, func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, elsewhere.URL+"/steal", http.StatusFound)
	})
	if _, err := c.Lookup(context.Background(), "974760673"); !errors.Is(err, ErrUnexpectedStatus) {
		t.Errorf("err = %v, want ErrUnexpectedStatus", err)
	}
	if leaked.Load() {
		t.Errorf("the service token was forwarded to the redirect target")
	}
}

func TestNewValidatesTheConfiguration(t *testing.T) {
	t.Parallel()

	for name, tc := range map[string]struct {
		cfg  Config
		want error
	}{
		"nothing":            {Config{}, ErrNotConfigured},
		"no token":           {Config{BaseURL: "http://org-core:8080"}, ErrNotConfigured},
		"no url":             {Config{Token: secret.Value(testToken)}, ErrNotConfigured},
		"short token":        {Config{BaseURL: "http://org-core:8080", Token: "short"}, ErrInvalidConfig},
		"placeholder token":  {Config{BaseURL: "http://org-core:8080", Token: secret.Value("change-me-" + testToken)}, ErrInvalidConfig},
		"not a url":          {Config{BaseURL: "org-core:8080", Token: secret.Value(testToken)}, ErrInvalidConfig},
		"credentials in url": {Config{BaseURL: "http://u:p@org-core:8080", Token: secret.Value(testToken)}, ErrInvalidConfig},
		"query in url":       {Config{BaseURL: "http://org-core:8080/?x=1", Token: secret.Value(testToken)}, ErrInvalidConfig},
		"bad principal":      {Config{BaseURL: "http://org-core:8080", Token: secret.Value(testToken), Principal: "anbud core"}, ErrInvalidConfig},
	} {
		if _, err := New(tc.cfg); !errors.Is(err, tc.want) {
			t.Errorf("%s: err = %v, want %v", name, err, tc.want)
		}
	}
}

func TestClientNeverPrintsItsToken(t *testing.T) {
	t.Parallel()

	c, err := New(Config{BaseURL: "http://org-core:8080", Token: secret.Value(testToken)})
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	for _, out := range []string{fmt.Sprintf("%v", c), fmt.Sprintf("%+v", c), fmt.Sprintf("%#v", c), fmt.Sprintf("%s", c)} {
		if strings.Contains(out, testToken) {
			t.Errorf("the client prints its token: %s", out)
		}
	}
}

func intPtr(n int) *int { return &n }

func equalOrg(a, b Organisation) bool {
	if (a.Employees == nil) != (b.Employees == nil) || (a.Employees != nil && *a.Employees != *b.Employees) {
		return false
	}
	a.Employees, b.Employees = nil, nil
	return a == b
}

// net/http quotes the whole URL in a transport error, and the URL holds the organisation number
// (for a sole trader, a person's business) and org-core's address. The error goes to a log.
func TestATransportErrorDoesNotQuoteTheURL(t *testing.T) {
	t.Parallel()

	c, srv := newServer(t, func(http.ResponseWriter, *http.Request) {})
	srv.Close() // nothing listens any more: the dial is refused
	_, err := c.Lookup(context.Background(), "974760673")
	if err == nil {
		t.Fatal("Lookup succeeded against a closed server")
	}
	for _, leaked := range []string{"974760673", "/api/v1/brreg", srv.URL} {
		if strings.Contains(err.Error(), leaked) {
			t.Errorf("the error quotes %q: %v", leaked, err)
		}
	}
	if !strings.Contains(err.Error(), "lookup request failed") {
		t.Errorf("the error does not say what failed: %v", err)
	}
}

func TestACancelledContextStillReadsAsCancelled(t *testing.T) {
	t.Parallel()

	c, _ := newServer(t, func(http.ResponseWriter, *http.Request) {})
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := c.Lookup(ctx, "974760673"); !errors.Is(err, context.Canceled) {
		t.Errorf("err = %v, want context.Canceled", err)
	}
}
