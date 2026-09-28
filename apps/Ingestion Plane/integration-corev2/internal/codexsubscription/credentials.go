package codexsubscription

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"sync"
	"time"

	"github.com/rs/zerolog"
)

const (
	codexAuthFileName     = "auth.json"
	credentialsLockName   = ".verevon-credentials.lock"
	defaultTokenURL       = "https://auth.openai.com/oauth/token"
	codexPublicClientID   = "app_EMoamEEZ73f0CkXaXp7hrann"
	defaultRefreshSkew    = 10 * time.Minute
	lastRefreshMaxAge     = 8 * 24 * time.Hour
	transientRefreshGrace = 30 * time.Second
	// transientRefreshBackoff is how long a connection whose refresh failed
	// transiently keeps its current token instead of calling the token
	// endpoint again, so an auth.openai.com outage cannot serialize every
	// call on the connection behind one slow refresh after another.
	transientRefreshBackoff = 30 * time.Second
	tokenEndpointTimeout    = 15 * time.Second
	maxTokenResponseBytes   = 64 << 10
	authReadAttempts        = 3
	authReadRetryDelay      = 50 * time.Millisecond
	credentialLockInterval  = 25 * time.Millisecond
	openAIAuthClaim         = "https://api.openai.com/auth"
	codexChatGPTAuthMode    = "chatgpt"
)

var (
	refreshClientIDPattern = regexp.MustCompile(`^app_[A-Za-z0-9]{8,64}$`)
	residencyPattern       = regexp.MustCompile(`^[A-Za-z0-9_-]{1,32}$`)
	accountIDPattern       = regexp.MustCompile(`^[A-Za-z0-9_.:-]{1,128}$`)
	errAuthUnparseable     = errors.New("codex auth.json is not parseable")

	permanentRefreshCodes = map[string]struct{}{
		"invalid_grant": {}, "refresh_token_expired": {}, "refresh_token_reused": {},
		"refresh_token_invalidated": {}, "invalid_client": {},
	}
)

// secret is a credential value that redacts itself everywhere it could be
// printed or serialized. reveal is used only where a header or form value is
// built.
type secret string

func (secret) String() string                      { return "[redacted]" }
func (secret) GoString() string                    { return "[redacted]" }
func (secret) MarshalJSON() ([]byte, error)        { return []byte(`"[redacted]"`), nil }
func (secret) MarshalText() ([]byte, error)        { return []byte("[redacted]"), nil }
func (secret) MarshalZerologObject(*zerolog.Event) {}
func (s secret) reveal() string                    { return string(s) }

// tokenSet is the usable view of one connection's Codex auth.json. Every
// representation of it is redacted: only reveal() on its secrets exposes a
// value, and only header and form builders call that.
type tokenSet struct {
	Access, Refresh, ID secret
	AccountID           string
	AccessExpiry        time.Time
	LastRefresh         time.Time
	ClientID            string
	Residency           string
	FedRAMP             bool
	accessFP, refreshFP [16]byte
}

func (tokenSet) String() string                      { return "tokenSet{[redacted]}" }
func (tokenSet) GoString() string                    { return "tokenSet{[redacted]}" }
func (tokenSet) MarshalZerologObject(*zerolog.Event) {}
func (tokenSet) MarshalJSON() ([]byte, error) {
	return []byte(`{"access":"[redacted]","refresh":"[redacted]","id":"[redacted]"}`), nil
}

// usableFor reports whether the access token stays valid for at least grace.
// An access token without an exp claim cannot be judged and counts as usable.
func (t tokenSet) usableFor(now time.Time, grace time.Duration) bool {
	return t.AccessExpiry.IsZero() || t.AccessExpiry.Sub(now) >= grace
}

func fingerprint(value string) [16]byte {
	sum := sha256.Sum256([]byte(value))
	var fp [16]byte
	copy(fp[:], sum[:16])
	return fp
}

// jsonMember and orderedObject keep a JSON object's keys in file order, so a
// write-back preserves every key the broker does not own, in place.
type jsonMember struct {
	Key   string
	Value json.RawMessage
}

type orderedObject []jsonMember

func parseOrderedObject(data []byte) (orderedObject, error) {
	decoder := json.NewDecoder(bytes.NewReader(data))
	token, err := decoder.Token()
	if err != nil {
		return nil, err
	}
	if delim, ok := token.(json.Delim); !ok || delim != '{' {
		return nil, errAuthUnparseable
	}
	object := orderedObject{}
	for decoder.More() {
		token, err := decoder.Token()
		if err != nil {
			return nil, err
		}
		key, ok := token.(string)
		if !ok {
			return nil, errAuthUnparseable
		}
		var value json.RawMessage
		if err := decoder.Decode(&value); err != nil {
			return nil, err
		}
		object = append(object, jsonMember{Key: key, Value: value})
	}
	if _, err := decoder.Token(); err != nil {
		return nil, err
	}
	if _, err := decoder.Token(); !errors.Is(err, io.EOF) {
		return nil, errAuthUnparseable
	}
	return object, nil
}

func (o orderedObject) get(key string) (json.RawMessage, bool) {
	for index := len(o) - 1; index >= 0; index-- {
		if o[index].Key == key {
			return o[index].Value, true
		}
	}
	return nil, false
}

func (o orderedObject) getString(key string) string {
	raw, ok := o.get(key)
	if !ok {
		return ""
	}
	var value string
	if json.Unmarshal(raw, &value) != nil {
		return ""
	}
	return value
}

// set replaces the last member named key in place, or appends it.
func (o *orderedObject) set(key string, value json.RawMessage) {
	for index := len(*o) - 1; index >= 0; index-- {
		if (*o)[index].Key == key {
			(*o)[index].Value = value
			return
		}
	}
	*o = append(*o, jsonMember{Key: key, Value: value})
}

func (o orderedObject) marshalCompact() ([]byte, error) {
	var buffer bytes.Buffer
	buffer.WriteByte('{')
	for index, member := range o {
		if index > 0 {
			buffer.WriteByte(',')
		}
		key, err := marshalJSONString(member.Key)
		if err != nil {
			return nil, err
		}
		buffer.Write(key)
		buffer.WriteByte(':')
		if err := json.Compact(&buffer, member.Value); err != nil {
			return nil, err
		}
	}
	buffer.WriteByte('}')
	return buffer.Bytes(), nil
}

// marshalJSONString encodes a string without HTML escaping, the way serde
// writes it.
func marshalJSONString(value string) ([]byte, error) {
	var buffer bytes.Buffer
	encoder := json.NewEncoder(&buffer)
	encoder.SetEscapeHTML(false)
	if err := encoder.Encode(value); err != nil {
		return nil, err
	}
	return bytes.TrimRight(buffer.Bytes(), "\n"), nil
}

// authSnapshot is one parse of auth.json: the whole ordered document, its
// ordered tokens object, the file mode, and the extracted token set. When an
// earlier write-back failed, it is the file with that unsaved grant laid
// over it.
type authSnapshot struct {
	document orderedObject
	tokens   orderedObject
	mode     os.FileMode
	set      tokenSet
	// diskRefreshFP fingerprints the refresh token the file itself holds.
	diskRefreshFP [16]byte
	// unsaved is the grant laid over the file; nil when the snapshot is the
	// file exactly as read.
	unsaved *unsavedGrant
}

// unsavedGrant is a refresh the token endpoint answered that auth.json could
// not store yet. The endpoint has already rotated the refresh token, so the
// grant is the only usable copy: it is served from memory and written again
// on the next access until the file holds it (§6.4, D7).
type unsavedGrant struct {
	// base fingerprints the refresh token of the file the grant replaces.
	base        [16]byte
	grant       refreshGrant
	refreshedAt time.Time
}

// transientFailure remembers that a connection's refresh failed transiently.
type transientFailure struct {
	refreshFP [16]byte
	until     time.Time
}

// credentialStore reads and refreshes the per-connection Codex auth.json. It
// is the only refresher of that file (§6): a per-connection lock, plus a
// file lock against a second broker process, makes refresh single-flight.
type credentialStore struct {
	tokenURL   string
	client     *http.Client
	skew       time.Duration
	now        func() time.Time
	userAgent  string
	originator string
	logger     zerolog.Logger
	// writeFile saves auth.json atomically; tests replace it to simulate a
	// failing disk.
	writeFile func(dir, path string, data []byte, mode os.FileMode) error

	locks sync.Map // codeHome -> chan struct{}

	mu            sync.Mutex
	permanentFail map[string][16]byte
	transientFail map[string]transientFailure
	unsaved       map[string]unsavedGrant
}

type credentialStoreConfig struct {
	TokenURL   string
	HTTPClient *http.Client
	Skew       time.Duration
	Now        func() time.Time
	UserAgent  string
	Originator string
	Logger     zerolog.Logger
}

func newCredentialStore(cfg credentialStoreConfig) *credentialStore {
	tokenURL := strings.TrimSpace(cfg.TokenURL)
	if tokenURL == "" {
		tokenURL = defaultTokenURL
	}
	skew := cfg.Skew
	if skew <= 0 {
		skew = defaultRefreshSkew
	}
	now := cfg.Now
	if now == nil {
		now = time.Now
	}
	client := cfg.HTTPClient
	if client == nil {
		client = &http.Client{Timeout: tokenEndpointTimeout}
	}
	originator := cfg.Originator
	if originator == "" {
		originator = defaultOriginator
	}
	userAgent := cfg.UserAgent
	if userAgent == "" {
		userAgent = directUserAgent()
	}
	return &credentialStore{
		tokenURL:      tokenURL,
		client:        withoutRedirects(client),
		skew:          skew,
		now:           now,
		userAgent:     userAgent,
		originator:    originator,
		logger:        cfg.Logger,
		writeFile:     writeFileAtomic,
		permanentFail: make(map[string][16]byte),
		transientFail: make(map[string]transientFailure),
		unsaved:       make(map[string]unsavedGrant),
	}
}

// withoutRedirects copies client so a credential never follows a redirect,
// whatever client the caller supplied.
func withoutRedirects(client *http.Client) *http.Client {
	copied := *client
	copied.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
	return &copied
}

// Current returns a token set that is fresh for at least the refresh skew,
// refreshing it first when needed. A missing or unusable login is
// ErrReauthenticationRequired.
func (s *credentialStore) Current(ctx context.Context, codeHome string) (tokenSet, error) {
	tokens, _, err := s.current(ctx, codeHome, time.Time{})
	return tokens, err
}

// current is Current plus whether this call refreshed the token. A non-zero
// need also refreshes an access token that expires by then, and keeps a
// token after a transient refresh failure only when it outlives need: the
// app-server fallback passes the end of Codex's own refresh window after the
// call, so the Codex process it spawns never refreshes by itself.
func (s *credentialStore) current(ctx context.Context, codeHome string, need time.Time) (tokenSet, bool, error) {
	snapshot, err := s.read(ctx, codeHome)
	if err != nil {
		return tokenSet{}, false, err
	}
	if snapshot.unsaved == nil {
		if !s.needsRefresh(snapshot.set, need) {
			return snapshot.set, false, nil
		}
		if s.inTransientBackoff(codeHome, snapshot.set.refreshFP) && s.usableUntil(snapshot.set, need) {
			// A refresh just failed transiently: keep the still-usable token
			// without queueing on the connection lock.
			return snapshot.set, false, nil
		}
	}
	var fresh tokenSet
	refreshed := false
	err = s.WithExclusive(ctx, codeHome, func() error {
		locked, err := s.read(ctx, codeHome)
		if err != nil {
			return err
		}
		if locked, err = s.retryUnsaved(codeHome, locked); err != nil {
			return err
		}
		if !s.needsRefresh(locked.set, need) {
			// Another goroutine or process refreshed while this one waited.
			fresh = locked.set
			return nil
		}
		fresh, refreshed, err = s.refreshLocked(ctx, codeHome, locked, false, need)
		return err
	})
	if err != nil {
		return tokenSet{}, false, err
	}
	return fresh, refreshed, nil
}

// recoverUnauthorized is the one-time recovery after an upstream 401 (§6.5):
// adopt a token someone else already refreshed, else force one refresh.
func (s *credentialStore) recoverUnauthorized(ctx context.Context, codeHome string, used tokenSet) (tokenSet, error) {
	var recovered tokenSet
	err := s.WithExclusive(ctx, codeHome, func() error {
		disk, err := s.read(ctx, codeHome)
		if err != nil {
			return err
		}
		if disk, err = s.retryUnsaved(codeHome, disk); err != nil {
			return err
		}
		if disk.set.AccountID != used.AccountID {
			return ErrReauthenticationRequired
		}
		if disk.set.accessFP != used.accessFP && disk.set.usableFor(s.now(), 0) {
			recovered = disk.set
			return nil
		}
		recovered, _, err = s.refreshLocked(ctx, codeHome, disk, true, time.Time{})
		return err
	})
	return recovered, err
}

// needsRefresh applies the refresh skew (§6.3) and, when need is set,
// refreshes an access token that expires by need, or, without an exp claim,
// one whose last refresh is older than Codex's interval by need.
func (s *credentialStore) needsRefresh(tokens tokenSet, need time.Time) bool {
	now := s.now()
	if !tokens.AccessExpiry.IsZero() {
		return tokens.AccessExpiry.Sub(now) <= s.skew || (!need.IsZero() && !tokens.AccessExpiry.After(need))
	}
	horizon := now
	if need.After(horizon) {
		horizon = need
	}
	return tokens.LastRefresh.IsZero() || horizon.Sub(tokens.LastRefresh) > lastRefreshMaxAge
}

// usableUntil reports whether the access token stays valid for at least the
// transient grace and, when need is set, until need.
func (s *credentialStore) usableUntil(tokens tokenSet, need time.Time) bool {
	floor := s.now().Add(transientRefreshGrace)
	if need.After(floor) {
		floor = need
	}
	return tokens.AccessExpiry.IsZero() || !tokens.AccessExpiry.Before(floor)
}

// WithExclusive runs fn while holding the connection's credential lock. The
// composed runner's Logout uses it, so a refresh can never write auth.json
// back after logout removed it.
func (s *credentialStore) WithExclusive(ctx context.Context, codeHome string, fn func() error) error {
	value, _ := s.locks.LoadOrStore(codeHome, make(chan struct{}, 1))
	lock := value.(chan struct{})
	select {
	case lock <- struct{}{}:
	case <-ctx.Done():
		return ctx.Err()
	}
	defer func() { <-lock }()
	release, err := lockCredentialFile(ctx, codeHome)
	if err != nil {
		return err
	}
	defer release()
	return fn()
}

// read is readFile with an unsaved grant laid over it. The overlay applies
// only while the file still holds the refresh token the grant replaced: a
// newer login or refresh on disk supersedes it, and a removed login (logout)
// drops it.
func (s *credentialStore) read(ctx context.Context, codeHome string) (authSnapshot, error) {
	snapshot, err := s.readFile(ctx, codeHome)
	pending, ok := s.unsavedFor(codeHome)
	if !ok {
		return snapshot, err
	}
	if err != nil {
		if errors.Is(err, ErrReauthenticationRequired) {
			s.dropUnsaved(codeHome, pending.base)
		}
		return snapshot, err
	}
	if snapshot.diskRefreshFP != pending.base {
		s.dropUnsaved(codeHome, pending.base)
		return snapshot, nil
	}
	overlaid, err := applyGrant(snapshot, pending.grant, pending.refreshedAt)
	if err != nil {
		return snapshot, nil
	}
	overlaid.unsaved = &pending
	return overlaid, nil
}

// readFile parses auth.json, retrying a torn read: Codex saves it by
// truncating and rewriting in place, so a concurrent read can briefly see a
// partial file.
func (s *credentialStore) readFile(ctx context.Context, codeHome string) (authSnapshot, error) {
	for attempt := 1; ; attempt++ {
		snapshot, err := readAuthSnapshot(codeHome)
		if !errors.Is(err, errAuthUnparseable) {
			return snapshot, err
		}
		if attempt >= authReadAttempts {
			return authSnapshot{}, ErrReauthenticationRequired
		}
		timer := time.NewTimer(authReadRetryDelay)
		select {
		case <-ctx.Done():
			timer.Stop()
			return authSnapshot{}, ctx.Err()
		case <-timer.C:
		}
	}
}

func readAuthSnapshot(codeHome string) (authSnapshot, error) {
	path := filepath.Join(codeHome, codexAuthFileName)
	info, err := os.Stat(path)
	if errors.Is(err, os.ErrNotExist) {
		return authSnapshot{}, ErrReauthenticationRequired
	}
	if err != nil {
		return authSnapshot{}, fmt.Errorf("inspect persisted Codex authentication: %w", err)
	}
	if !info.Mode().IsRegular() {
		return authSnapshot{}, ErrReauthenticationRequired
	}
	data, err := os.ReadFile(path)
	if errors.Is(err, os.ErrNotExist) {
		return authSnapshot{}, ErrReauthenticationRequired
	}
	if err != nil {
		return authSnapshot{}, fmt.Errorf("read persisted Codex authentication: %w", err)
	}
	if len(bytes.TrimSpace(data)) == 0 {
		return authSnapshot{}, errAuthUnparseable
	}
	document, err := parseOrderedObject(data)
	if err != nil {
		return authSnapshot{}, errAuthUnparseable
	}
	if raw, present := document.get("auth_mode"); present && string(raw) != "null" {
		if document.getString("auth_mode") != codexChatGPTAuthMode {
			return authSnapshot{}, ErrReauthenticationRequired
		}
	}
	rawTokens, present := document.get("tokens")
	if !present || string(rawTokens) == "null" {
		return authSnapshot{}, ErrReauthenticationRequired
	}
	tokens, err := parseOrderedObject(rawTokens)
	if err != nil {
		return authSnapshot{}, ErrReauthenticationRequired
	}
	set, err := extractTokenSet(document, tokens)
	if err != nil {
		return authSnapshot{}, err
	}
	return authSnapshot{document: document, tokens: tokens, mode: info.Mode(), set: set, diskRefreshFP: set.refreshFP}, nil
}

func extractTokenSet(document, tokens orderedObject) (tokenSet, error) {
	access := strings.TrimSpace(tokens.getString("access_token"))
	refresh := strings.TrimSpace(tokens.getString("refresh_token"))
	if access == "" || refresh == "" {
		return tokenSet{}, ErrReauthenticationRequired
	}
	idToken := strings.TrimSpace(tokens.getString("id_token"))
	accessClaims := jwtClaims(access)
	idClaims := jwtClaims(idToken)
	set := tokenSet{
		Access:    secret(access),
		Refresh:   secret(refresh),
		ID:        secret(idToken),
		accessFP:  fingerprint(access),
		refreshFP: fingerprint(refresh),
		ClientID:  codexPublicClientID,
	}
	if exp, ok := numericClaim(accessClaims, "exp"); ok && exp > 0 {
		set.AccessExpiry = time.Unix(int64(exp), 0).UTC()
	}
	if raw := document.getString("last_refresh"); raw != "" {
		if parsed, err := time.Parse(time.RFC3339Nano, raw); err == nil {
			set.LastRefresh = parsed
		}
	}
	if clientID := stringClaim(accessClaims, "client_id"); refreshClientIDPattern.MatchString(clientID) {
		set.ClientID = clientID
	}
	for _, candidate := range []string{
		tokens.getString("account_id"),
		stringClaim(authClaims(idClaims), "chatgpt_account_id"),
		stringClaim(authClaims(accessClaims), "chatgpt_account_id"),
	} {
		if candidate = strings.TrimSpace(candidate); accountIDPattern.MatchString(candidate) {
			set.AccountID = candidate
			break
		}
	}
	residency := stringClaim(authClaims(accessClaims), "chatgpt_compute_residency")
	if residency == "" {
		residency = stringClaim(accessClaims, openAIAuthClaim+".chatgpt_compute_residency")
	}
	if residency == "" {
		residency = stringClaim(accessClaims, "chatgpt_compute_residency")
	}
	if residencyPattern.MatchString(residency) && residency != "no_constraint" {
		set.Residency = residency
	}
	set.FedRAMP = boolClaim(authClaims(idClaims), "chatgpt_account_is_fedramp")
	return set, nil
}

// jwtClaims decodes a JWT payload without checking its signature: the claims
// only route the request, they never authorize anything here.
func jwtClaims(token string) map[string]json.RawMessage {
	parts := strings.Split(token, ".")
	if len(parts) < 2 {
		return nil
	}
	payload, err := base64.RawURLEncoding.DecodeString(strings.TrimRight(parts[1], "="))
	if err != nil {
		return nil
	}
	var claims map[string]json.RawMessage
	if json.Unmarshal(payload, &claims) != nil {
		return nil
	}
	return claims
}

func authClaims(claims map[string]json.RawMessage) map[string]json.RawMessage {
	raw, ok := claims[openAIAuthClaim]
	if !ok {
		return nil
	}
	var nested map[string]json.RawMessage
	if json.Unmarshal(raw, &nested) != nil {
		return nil
	}
	return nested
}

func stringClaim(claims map[string]json.RawMessage, key string) string {
	var value string
	if raw, ok := claims[key]; !ok || json.Unmarshal(raw, &value) != nil {
		return ""
	}
	return strings.TrimSpace(value)
}

func numericClaim(claims map[string]json.RawMessage, key string) (float64, bool) {
	var value float64
	if raw, ok := claims[key]; !ok || json.Unmarshal(raw, &value) != nil {
		return 0, false
	}
	return value, true
}

func boolClaim(claims map[string]json.RawMessage, key string) bool {
	var value bool
	if raw, ok := claims[key]; !ok || json.Unmarshal(raw, &value) != nil {
		return false
	}
	return value
}

// refreshGrant is a token endpoint 200 response. Omitted optional tokens
// keep their old values.
type refreshGrant struct {
	access, refresh, id string
}

// then is g followed by a later grant: the later grant's values win, and a
// value it omits keeps g's.
func (g refreshGrant) then(later refreshGrant) refreshGrant {
	merged := g
	if later.access != "" {
		merged.access = later.access
	}
	if later.refresh != "" {
		merged.refresh = later.refresh
	}
	if later.id != "" {
		merged.id = later.id
	}
	return merged
}

type permanentRefreshError struct{ code string }

func (e *permanentRefreshError) Error() string {
	return "ChatGPT refresh token was rejected (" + e.code + ")"
}

type transientRefreshError struct{ reason string }

func (e *transientRefreshError) Error() string {
	return "ChatGPT token refresh is temporarily unavailable (" + e.reason + ")"
}

// refreshLocked runs steps 4 to 8 of §6.4 with the connection lock held. It
// reports whether it actually replaced the token. forced is the 401
// recovery, where keeping the rejected token is never an option; need is as
// in current.
func (s *credentialStore) refreshLocked(ctx context.Context, codeHome string, snapshot authSnapshot, forced bool, need time.Time) (tokenSet, bool, error) {
	current := snapshot.set
	if s.permanentlyFailed(codeHome, current.refreshFP) {
		return tokenSet{}, false, ErrReauthenticationRequired
	}
	if s.inTransientBackoff(codeHome, current.refreshFP) {
		// A refresh of this very token failed transiently moments ago; do
		// not queue another slow attempt behind it.
		return s.afterTransientFailure(current, forced, need, "refresh_backoff", false)
	}
	grant, err := s.requestRefresh(ctx, current)
	if err != nil {
		var permanent *permanentRefreshError
		if errors.As(err, &permanent) {
			s.markPermanentFailure(codeHome, current.refreshFP)
			s.logger.Warn().Str("event", "refresh_failed_permanent").Str("error_code", permanent.code).Msg("codex_subscription_credentials")
			return tokenSet{}, false, ErrReauthenticationRequired
		}
		var transient *transientRefreshError
		reason := "other"
		if errors.As(err, &transient) {
			reason = transient.reason
		}
		if reason != "caller_gone" {
			s.markTransientFailure(codeHome, current.refreshFP)
		}
		if ctxErr := ctx.Err(); ctxErr != nil {
			return tokenSet{}, false, fmt.Errorf("refresh ChatGPT subscription credential: %w", ctxErr)
		}
		return s.afterTransientFailure(current, forced, need, reason, true)
	}
	s.clearTransientFailure(codeHome)
	refreshedAt := s.now().UTC()
	updated, err := applyGrant(snapshot, grant, refreshedAt)
	if err != nil {
		return tokenSet{}, false, err
	}
	// The endpoint has rotated the refresh token: keep the grant in memory
	// until the file holds it, so a failing write can never lose it.
	merged := grant
	if snapshot.unsaved != nil {
		merged = snapshot.unsaved.grant.then(grant)
	}
	pending := unsavedGrant{base: snapshot.diskRefreshFP, grant: merged, refreshedAt: refreshedAt}
	updated.unsaved = &pending
	s.setUnsaved(codeHome, pending)
	saved, err := s.retryUnsaved(codeHome, updated)
	if err != nil {
		return tokenSet{}, false, err
	}
	return saved.set, true, nil
}

// afterTransientFailure keeps the current token when it is still usable (and
// the refresh was not forced), else reports the refresh as unavailable.
func (s *credentialStore) afterTransientFailure(current tokenSet, forced bool, need time.Time, reason string, attempted bool) (tokenSet, bool, error) {
	if !forced && s.usableUntil(current, need) {
		if attempted {
			s.logger.Warn().Str("event", "refresh_failed_transient").Str("error_code", sanitizeUpstreamCode(reason)).Msg("codex_subscription_credentials")
		}
		return current, false, nil
	}
	return tokenSet{}, false, &UpstreamError{Kind: upstreamKindRefresh, Code: sanitizeUpstreamCode(reason), Retryable: false}
}

// retryUnsaved writes an unsaved grant to auth.json. It returns the snapshot
// as saved, or, while the write keeps failing, the in-memory snapshot, so
// the rotated token stays in use. A login that logout removed is never
// recreated.
func (s *credentialStore) retryUnsaved(codeHome string, snapshot authSnapshot) (authSnapshot, error) {
	if snapshot.unsaved == nil {
		return snapshot, nil
	}
	base := snapshot.unsaved.base
	saved, err := s.persist(codeHome, snapshot)
	switch {
	case err == nil:
		s.dropUnsaved(codeHome, base)
		return saved, nil
	case errors.Is(err, ErrReauthenticationRequired):
		s.dropUnsaved(codeHome, base)
		return authSnapshot{}, err
	}
	s.logger.Warn().Str("event", "write_back_failed").Msg("codex_subscription_credentials")
	return snapshot, nil
}

func (s *credentialStore) permanentlyFailed(codeHome string, refreshFP [16]byte) bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	failed, ok := s.permanentFail[codeHome]
	return ok && failed == refreshFP
}

func (s *credentialStore) markPermanentFailure(codeHome string, refreshFP [16]byte) {
	s.mu.Lock()
	s.permanentFail[codeHome] = refreshFP
	s.mu.Unlock()
}

func (s *credentialStore) inTransientBackoff(codeHome string, refreshFP [16]byte) bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	failed, ok := s.transientFail[codeHome]
	if !ok {
		return false
	}
	if failed.refreshFP != refreshFP || !s.now().Before(failed.until) {
		delete(s.transientFail, codeHome)
		return false
	}
	return true
}

func (s *credentialStore) markTransientFailure(codeHome string, refreshFP [16]byte) {
	s.mu.Lock()
	s.transientFail[codeHome] = transientFailure{refreshFP: refreshFP, until: s.now().Add(transientRefreshBackoff)}
	s.mu.Unlock()
}

func (s *credentialStore) clearTransientFailure(codeHome string) {
	s.mu.Lock()
	delete(s.transientFail, codeHome)
	s.mu.Unlock()
}

func (s *credentialStore) unsavedFor(codeHome string) (unsavedGrant, bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	pending, ok := s.unsaved[codeHome]
	return pending, ok
}

func (s *credentialStore) setUnsaved(codeHome string, pending unsavedGrant) {
	s.mu.Lock()
	s.unsaved[codeHome] = pending
	s.mu.Unlock()
}

// dropUnsaved forgets the unsaved grant that replaced base, leaving a newer
// one in place.
func (s *credentialStore) dropUnsaved(codeHome string, base [16]byte) {
	s.mu.Lock()
	if pending, ok := s.unsaved[codeHome]; ok && pending.base == base {
		delete(s.unsaved, codeHome)
	}
	s.mu.Unlock()
}

// requestRefresh posts the RFC 6749 §6 refresh grant. The token endpoint
// receives only the refresh token; the access token never goes there.
//
// Once sent, the request is detached from the caller: the endpoint may
// rotate the refresh token as soon as it receives it, so its answer must be
// read and stored even when the caller gave up or its deadline passed.
// Dropping it would leave only the consumed refresh token, whose reuse
// revokes the whole token family (D7).
func (s *credentialStore) requestRefresh(ctx context.Context, current tokenSet) (refreshGrant, error) {
	if ctx.Err() != nil {
		// Nothing was sent yet, so nothing can have rotated.
		return refreshGrant{}, &transientRefreshError{reason: "caller_gone"}
	}
	refreshCtx, cancel := context.WithTimeout(context.WithoutCancel(ctx), tokenEndpointTimeout)
	defer cancel()
	form := url.Values{
		"grant_type":    {"refresh_token"},
		"refresh_token": {current.Refresh.reveal()},
		"client_id":     {current.ClientID},
	}
	request, err := http.NewRequestWithContext(refreshCtx, http.MethodPost, s.tokenURL, strings.NewReader(form.Encode()))
	if err != nil {
		return refreshGrant{}, &transientRefreshError{reason: "other"}
	}
	request.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	request.Header.Set("Accept", "application/json")
	request.Header.Set("User-Agent", s.userAgent)
	request.Header.Set("originator", s.originator)
	response, err := s.client.Do(request)
	if err != nil {
		return refreshGrant{}, &transientRefreshError{reason: transportErrorCode(err)}
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, maxTokenResponseBytes))
	if err != nil {
		return refreshGrant{}, &transientRefreshError{reason: "eof"}
	}
	if response.StatusCode == http.StatusOK {
		var grant struct {
			AccessToken  string `json:"access_token"`
			RefreshToken string `json:"refresh_token"`
			IDToken      string `json:"id_token"`
		}
		if json.Unmarshal(body, &grant) != nil || strings.TrimSpace(grant.AccessToken) == "" {
			return refreshGrant{}, &transientRefreshError{reason: "other"}
		}
		return refreshGrant{
			access:  strings.TrimSpace(grant.AccessToken),
			refresh: strings.TrimSpace(grant.RefreshToken),
			id:      strings.TrimSpace(grant.IDToken),
		}, nil
	}
	code := sanitizeUpstreamCode(parseUpstreamErrorBody(body).Code)
	if _, permanent := permanentRefreshCodes[code]; permanent || response.StatusCode == http.StatusUnauthorized {
		if code == "" {
			code = "unauthorized"
		}
		return refreshGrant{}, &permanentRefreshError{code: code}
	}
	reason := "http_" + httpStatusClass(response.StatusCode)
	if response.StatusCode == http.StatusTooManyRequests {
		reason = "rate_limit_exceeded"
	} else if response.StatusCode >= 500 {
		reason = "server_error"
	}
	return refreshGrant{}, &transientRefreshError{reason: reason}
}

func httpStatusClass(status int) string {
	switch {
	case status >= 500:
		return "5xx"
	case status >= 400:
		return "4xx"
	default:
		return "other"
	}
}

// applyGrant replaces the rotated values in the snapshot's document, in
// place (§6.4 steps 6 and 7.2), without touching the file. Omitted optional
// tokens keep their old values, and account_id is left as it is.
func applyGrant(snapshot authSnapshot, grant refreshGrant, refreshedAt time.Time) (authSnapshot, error) {
	tokens := append(orderedObject{}, snapshot.tokens...)
	document := append(orderedObject{}, snapshot.document...)
	values := []struct {
		key, value string
	}{{"access_token", grant.access}, {"id_token", grant.id}, {"refresh_token", grant.refresh}}
	for _, item := range values {
		if item.value == "" {
			continue
		}
		encoded, err := marshalJSONString(item.value)
		if err != nil {
			return authSnapshot{}, err
		}
		tokens.set(item.key, encoded)
	}
	tokensJSON, err := tokens.marshalCompact()
	if err != nil {
		return authSnapshot{}, err
	}
	document.set("tokens", tokensJSON)
	lastRefresh, err := marshalJSONString(refreshedAt.UTC().Format(time.RFC3339Nano))
	if err != nil {
		return authSnapshot{}, err
	}
	document.set("last_refresh", lastRefresh)
	set, err := extractTokenSet(document, tokens)
	if err != nil {
		return authSnapshot{}, err
	}
	return authSnapshot{document: document, tokens: tokens, mode: snapshot.mode, set: set, diskRefreshFP: snapshot.diskRefreshFP}, nil
}

// persist saves the snapshot's document atomically in the Codex layout
// (§6.4 step 7) and returns it as the file now holds it.
func (s *credentialStore) persist(codeHome string, snapshot authSnapshot) (authSnapshot, error) {
	path := filepath.Join(codeHome, codexAuthFileName)
	info, err := os.Stat(path)
	if errors.Is(err, os.ErrNotExist) {
		// Logout removed the login while the token endpoint answered. Never
		// recreate it.
		return authSnapshot{}, ErrReauthenticationRequired
	}
	if err != nil {
		return authSnapshot{}, fmt.Errorf("inspect persisted Codex authentication: %w", err)
	}
	compact, err := snapshot.document.marshalCompact()
	if err != nil {
		return authSnapshot{}, err
	}
	var pretty bytes.Buffer
	if err := json.Indent(&pretty, compact, "", "  "); err != nil {
		return authSnapshot{}, err
	}
	mode := info.Mode().Perm() & 0o600
	if mode&0o400 == 0 {
		mode = 0o600
	}
	if err := s.writeFile(codeHome, path, pretty.Bytes(), mode); err != nil {
		return authSnapshot{}, err
	}
	saved := snapshot
	saved.mode = mode
	saved.diskRefreshFP = snapshot.set.refreshFP
	saved.unsaved = nil
	return saved, nil
}

func writeFileAtomic(dir, path string, data []byte, mode os.FileMode) (err error) {
	temp, err := os.CreateTemp(dir, ".auth.json.*.tmp")
	if err != nil {
		return fmt.Errorf("create Codex authentication temp file: %w", err)
	}
	tempName := temp.Name()
	defer func() {
		if err != nil {
			_ = temp.Close()
			_ = os.Remove(tempName)
		}
	}()
	if err = temp.Chmod(mode); err != nil {
		return fmt.Errorf("protect Codex authentication temp file: %w", err)
	}
	if _, err = temp.Write(data); err != nil {
		return fmt.Errorf("write Codex authentication temp file: %w", err)
	}
	if err = temp.Sync(); err != nil {
		return fmt.Errorf("sync Codex authentication temp file: %w", err)
	}
	if err = temp.Close(); err != nil {
		return fmt.Errorf("close Codex authentication temp file: %w", err)
	}
	if err = replaceFile(tempName, path); err != nil {
		return fmt.Errorf("replace Codex authentication: %w", err)
	}
	syncDirectory(dir)
	return nil
}
