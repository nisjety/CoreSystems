package cost

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/golang-jwt/jwt/v5"
)

// One bearer per organization, cached until shortly before it expires.
func TestTheBearerIsMintedOncePerOrganizationAndReusedUntilItIsAboutToExpire(t *testing.T) {
	rig := newForwardRig(t, []string{"org-a", "org-b"})
	ctx := context.Background()

	first, err := rig.tokens.Token(ctx, "org-a")
	if err != nil {
		t.Fatalf("mint: %v", err)
	}
	for i := 0; i < 5; i++ {
		again, err := rig.tokens.Token(ctx, "org-a")
		if err != nil || again != first {
			t.Fatalf("call %d returned a different bearer or an error: %v", i+2, err)
		}
	}
	if got := rig.auth.mintCount("org-a"); got != 1 {
		t.Fatalf("org-a was minted %d times for six uses, want 1", got)
	}

	other, err := rig.tokens.Token(ctx, "org-b")
	if err != nil || other == first {
		t.Fatalf("org-b got org-a's bearer (or an error: %v)", err)
	}
	if rig.auth.mintCount("org-b") != 1 {
		t.Fatalf("org-b minted %d times", rig.auth.mintCount("org-b"))
	}

	// A five-minute bearer is reused for a quarter of its life less than that: one minute
	// short of the end, never one that a request lasting seconds could outlive.
	rig.clock.advance(3*time.Minute + 59*time.Second)
	if again, _ := rig.tokens.Token(ctx, "org-a"); again != first {
		t.Fatal("a bearer with a minute to live was replaced early")
	}
	rig.clock.advance(2 * time.Second) // 4m01s: inside the last minute
	renewed, err := rig.tokens.Token(ctx, "org-a")
	if err != nil || renewed == first {
		t.Fatalf("a bearer about to expire was reused (err %v)", err)
	}
	if rig.auth.mintCount("org-a") != 2 {
		t.Fatalf("org-a minted %d times, want 2", rig.auth.mintCount("org-a"))
	}
}

func TestAnInvalidatedBearerIsReplacedAndAFailedMintIsNotCached(t *testing.T) {
	rig := newForwardRig(t, []string{"org-a"})
	ctx := context.Background()
	if _, err := rig.tokens.Token(ctx, "org-a"); err != nil {
		t.Fatal(err)
	}
	rig.tokens.Invalidate("org-a")
	if _, err := rig.tokens.Token(ctx, "org-a"); err != nil || rig.auth.mintCount("org-a") != 2 {
		t.Fatalf("after invalidation: err=%v mints=%d, want a second mint", err, rig.auth.mintCount("org-a"))
	}

	rig.tokens.Invalidate("org-a")
	rig.auth.mu.Lock()
	rig.auth.status = http.StatusBadGateway
	rig.auth.mu.Unlock()
	if _, err := rig.tokens.Token(ctx, "org-a"); err == nil {
		t.Fatal("a failed mint returned a bearer")
	}
	rig.auth.mu.Lock()
	rig.auth.status = 0
	rig.auth.mu.Unlock()
	if _, err := rig.tokens.Token(ctx, "org-a"); err != nil {
		t.Fatalf("the failure was cached: %v", err)
	}
}

// What Auth Core answers is not trusted blindly: a bearer that is not the one asked for is not
// used. (cost-core verifies the signature; this is what can be checked without the key.)
func TestABearerThatIsNotTheOneAskedForIsNeverUsed(t *testing.T) {
	ctx := context.Background()
	t.Run("another organization's bearer", func(t *testing.T) {
		rig := newForwardRig(t, []string{"org-a", "org-b"})
		rig.auth.wrong = "org-b"
		if _, err := rig.tokens.Token(ctx, "org-a"); err == nil {
			t.Fatal("a bearer for org-b was accepted for org-a")
		}
	})
	forge := func(t *testing.T, edit func(jwt.MapClaims)) *AuthCoreTokens {
		t.Helper()
		rig := newForwardRig(t, []string{"org-a"})
		stand := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			now := time.Now()
			claims := jwt.MapClaims{
				"iss": rig.auth.issuer, "aud": "cost-core", "sub": "service:data-orchestrator", "principal_type": "service",
				"org_id": "org-a", "scopes": []string{"cost:write"}, "iat": now.Unix(), "exp": now.Add(5 * time.Minute).Unix(),
			}
			edit(claims)
			signed, _ := jwt.NewWithClaims(jwt.SigningMethodRS256, claims).SignedString(rigKey)
			_ = json.NewEncoder(w).Encode(map[string]string{"token": signed})
		}))
		t.Cleanup(stand.Close)
		tokens, err := NewAuthCoreTokens(stand.URL, "data-orchestrator", rig.auth.apiKey, rig.auth.issuer, nil)
		if err != nil {
			t.Fatal(err)
		}
		return tokens
	}
	for name, edit := range map[string]func(jwt.MapClaims){
		"another audience":             func(c jwt.MapClaims) { c["aud"] = "inference-core" },
		"two audiences":                func(c jwt.MapClaims) { c["aud"] = []string{"cost-core", "inference-core"} },
		"a user principal":             func(c jwt.MapClaims) { c["principal_type"] = "user" },
		"another service":              func(c jwt.MapClaims) { c["sub"] = "service:model-gateway" },
		"a broader scope":              func(c jwt.MapClaims) { c["scopes"] = []string{"cost:write", "cost:read"} },
		"a read scope":                 func(c jwt.MapClaims) { c["scopes"] = []string{"cost:read"} },
		"no scope":                     func(c jwt.MapClaims) { delete(c, "scopes") },
		"another issuer":               func(c jwt.MapClaims) { c["iss"] = "http://elsewhere/api/convex-auth" },
		"a lifetime over five minutes": func(c jwt.MapClaims) { c["exp"] = time.Now().Add(time.Hour).Unix() },
		"already expired": func(c jwt.MapClaims) {
			c["exp"] = time.Now().Add(-time.Minute).Unix()
			c["iat"] = time.Now().Add(-time.Hour).Unix()
		},
		"issued in the future": func(c jwt.MapClaims) {
			c["iat"] = time.Now().Add(time.Hour).Unix()
			c["exp"] = time.Now().Add(time.Hour + time.Minute).Unix()
		},
		"another organization": func(c jwt.MapClaims) { c["org_id"] = "org-b" },
	} {
		t.Run(name, func(t *testing.T) {
			if _, err := forge(t, edit).Token(ctx, "org-a"); err == nil {
				t.Fatal("a bearer that is not the one asked for was accepted")
			}
		})
	}
	// And the control: the same forger with no edit is accepted, so each refusal above is
	// for its own reason.
	if _, err := forge(t, func(jwt.MapClaims) {}).Token(ctx, "org-a"); err != nil {
		t.Fatalf("the unedited bearer was refused: %v", err)
	}
}

// What is sent to Auth Core: the principal's own id and credential in headers, the
// organization, the one scope and a reason in the body, and nothing else.
func TestTheMintRequestCarriesTheCredentialInHeadersAndOnlyTheOrganizationScopeAndReason(t *testing.T) {
	var gotHeaders http.Header
	var gotBody map[string]any
	stand := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotHeaders = r.Header.Clone()
		_ = json.NewDecoder(r.Body).Decode(&gotBody)
		http.Error(w, "no", http.StatusForbidden)
	}))
	defer stand.Close()
	key := strings.Repeat("s", 48)
	tokens, err := NewAuthCoreTokens(stand.URL+"/api/cost-core/internal-token", "data-orchestrator", key, "", nil)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := tokens.Token(context.Background(), "org-a"); err == nil {
		t.Fatal("a refused mint returned a bearer")
	} else if strings.Contains(err.Error(), key) {
		t.Fatal("the error carries the service credential")
	}
	if gotHeaders.Get("x-service-id") != "data-orchestrator" || gotHeaders.Get("x-service-api-key") != key {
		t.Fatalf("headers = %v", gotHeaders)
	}
	if len(gotBody) != 3 || gotBody["orgId"] != "org-a" || gotBody["reason"] != "usage forward" {
		t.Fatalf("body = %v", gotBody)
	}
	if scopes, _ := gotBody["scopes"].([]any); len(scopes) != 1 || scopes[0] != "cost:write" {
		t.Fatalf("scopes = %v", gotBody["scopes"])
	}
	// No retention posture is asked for: Auth Core selects it and refuses a request that names one.
	if _, present := gotBody["zdr"]; present || gotHeaders.Get("x-zdr") != "" {
		t.Fatal("the mint request names a retention posture")
	}
}

func TestTheTokenClientRefusesIncompleteConfigurationAndOversizedAnswers(t *testing.T) {
	for name, args := range map[string][3]string{
		"no url":        {"", "data-orchestrator", strings.Repeat("k", 40)},
		"no service id": {"http://auth", "", strings.Repeat("k", 40)},
		"a short key":   {"http://auth", "data-orchestrator", "short"},
		"a blank key":   {"http://auth", "data-orchestrator", strings.Repeat(" ", 40)},
	} {
		if _, err := NewAuthCoreTokens(args[0], args[1], args[2], "", nil); err == nil {
			t.Errorf("%s: a token client was built", name)
		}
	}
	huge := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte(`{"token":"` + base64.RawURLEncoding.EncodeToString([]byte(strings.Repeat("x", 100_000))) + `"}`))
	}))
	defer huge.Close()
	tokens, _ := NewAuthCoreTokens(huge.URL, "data-orchestrator", strings.Repeat("k", 40), "", nil)
	if _, err := tokens.Token(context.Background(), "org-a"); err == nil {
		t.Fatal("an oversized answer was accepted")
	}
	if _, err := tokens.Token(context.Background(), "  "); err == nil {
		t.Fatal("a blank organization got a bearer")
	}
}

// fixedBearer is a TokenSource that hands every organization the same bearer.
type fixedBearer struct{ bearer string }

func (f fixedBearer) Token(context.Context, string) (string, error) { return f.bearer, nil }
func (f fixedBearer) Invalidate(string)                             {}

// A redirect is an answer, not an instruction. The mint carries the principal's credential in a
// custom header, and Go forwards custom headers to whatever host a redirect names (it strips
// only Authorization and cookies across domains), so a client that followed one would hand the
// credential to another address; the forwarder's bearer would follow a same-host redirect the
// same way. Neither client follows.
func TestARedirectIsNeverFollowedWithTheServiceCredentialOrTheBearer(t *testing.T) {
	var hits atomic.Int32
	elsewhere := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		hits.Add(1)
		w.WriteHeader(http.StatusAccepted)
	}))
	defer elsewhere.Close()
	redirecting := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, elsewhere.URL+"/collect", http.StatusTemporaryRedirect)
	}))
	defer redirecting.Close()

	t.Run("the mint", func(t *testing.T) {
		hits.Store(0)
		tokens, err := NewAuthCoreTokens(redirecting.URL, "data-orchestrator", strings.Repeat("s", 48), "", nil)
		if err != nil {
			t.Fatal(err)
		}
		if _, err := tokens.Token(context.Background(), "org-a"); err == nil {
			t.Fatal("a redirect produced a bearer")
		}
		if hits.Load() != 0 {
			t.Fatal("the mint followed a redirect and sent the service credential to another address")
		}
	})

	t.Run("the post to cost-core", func(t *testing.T) {
		hits.Store(0)
		queue := newMemoryQueue(time.Now, usageRow("org-a", "contextual-retrieval", "redirect"))
		forwarder, err := NewForwarder(queue, fixedBearer{bearer: "bearer-not-for-anyone-else"}, nil, ForwarderConfig{BaseURL: redirecting.URL})
		if err != nil {
			t.Fatal(err)
		}
		result, err := forwarder.RunOnce(context.Background())
		if err != nil || result.Forwarded != 0 || result.Failed != 1 {
			t.Fatalf("pass = %+v, err = %v: a redirect is not an acceptance", result, err)
		}
		if hits.Load() != 0 {
			t.Fatal("the forwarder followed a redirect with the organization's bearer")
		}
		if state := queue.state(1); state.forwarded || state.class != failRejected {
			t.Fatalf("the row after a redirect: forwarded=%v class=%q, want unforwarded and %q", state.forwarded, state.class, failRejected)
		}
	})
}
