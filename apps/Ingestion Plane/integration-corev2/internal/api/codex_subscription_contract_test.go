package api

// Pins the ChatGPT-subscription reason vocabulary between this broker and
// Model Plane's inference-core, which reads the broker's replies.
//
// The two are different languages and deploy separately, so inference-core
// spells every reason code and error field as its own literal. A silent
// disagreement fails nothing: a code or field renamed on one side only makes
// inference-core fall back to its generic Unavailable, and the chat UI hides
// the reason (a usage limit's reset time, a needed reconnect) again. Model
// Gateway's subscription_failure_contract.rs pins inference-core to the
// gateway; this test reads inference-core's source and pins it to the broker.

import (
	"encoding/json"
	"go/ast"
	"go/parser"
	"go/token"
	"net/http"
	"os"
	"path/filepath"
	"reflect"
	"regexp"
	"slices"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/integration-corev2/internal/codexsubscription"
)

// modelPlaneDir is apps/Model Plane, seen from this package's directory.
var modelPlaneDir = filepath.Join("..", "..", "..", "..", "Model Plane")

// inferenceCoreBrokerSource is inference-core's broker adapter. The test is
// skipped only when the whole Model Plane tree is absent (this module checked
// out or built on its own); a moved file fails it.
func inferenceCoreBrokerSource(t *testing.T) string {
	t.Helper()
	if _, err := os.Stat(modelPlaneDir); err != nil {
		t.Skipf("Model Plane is not beside Ingestion Plane (%v); the broker/inference-core contract cannot be checked here", err)
	}
	path := filepath.Join(modelPlaneDir, "rust", "services", "inference-core", "src", "provider", "codex_subscription.rs")
	source, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("cannot read %s (%v). If the file moved, re-point this test; do not delete it: it is what keeps the broker and inference-core reading one reason vocabulary.", path, err)
	}
	return string(source)
}

// brokerFailureCodes is every constant of type FailureCode the
// codexsubscription package declares, by name, read from its source so that
// a code added there is checked without editing this test.
func brokerFailureCodes(t *testing.T) map[string]string {
	t.Helper()
	paths, err := filepath.Glob(filepath.Join("..", "codexsubscription", "*.go"))
	if err != nil || len(paths) == 0 {
		t.Fatalf("list codexsubscription sources: %v (%d files)", err, len(paths))
	}
	codes := map[string]string{}
	fileSet := token.NewFileSet()
	for _, path := range paths {
		if strings.HasSuffix(path, "_test.go") {
			continue
		}
		file, err := parser.ParseFile(fileSet, path, nil, 0)
		if err != nil {
			t.Fatalf("parse %s: %v", path, err)
		}
		for _, decl := range file.Decls {
			general, ok := decl.(*ast.GenDecl)
			if !ok || general.Tok != token.CONST {
				continue
			}
			for _, spec := range general.Specs {
				value, ok := spec.(*ast.ValueSpec)
				if !ok {
					continue
				}
				if typeName, ok := value.Type.(*ast.Ident); !ok || typeName.Name != "FailureCode" {
					continue
				}
				for index, name := range value.Names {
					literal, ok := value.Values[index].(*ast.BasicLit)
					if !ok || literal.Kind != token.STRING {
						t.Fatalf("%s: FailureCode %s is not a string literal", path, name.Name)
					}
					unquoted, err := strconv.Unquote(literal.Value)
					if err != nil {
						t.Fatalf("%s: FailureCode %s: %v", path, name.Name, err)
					}
					codes[name.Name] = unquoted
				}
			}
		}
	}
	// The parser read what the compiler sees.
	for name, code := range map[string]codexsubscription.FailureCode{
		"FailureUsageLimit":          codexsubscription.FailureUsageLimit,
		"FailureReauthRequired":      codexsubscription.FailureReauthRequired,
		"FailureUpstreamUnavailable": codexsubscription.FailureUpstreamUnavailable,
		"FailureContextTooLong":      codexsubscription.FailureContextTooLong,
		"FailureBadRequest":          codexsubscription.FailureBadRequest,
		"FailureRateLimited":         codexsubscription.FailureRateLimited,
		"FailureBrokerFailed":        codexsubscription.FailureBrokerFailed,
	} {
		if codes[name] != string(code) {
			t.Fatalf("parsed %s = %q, want %q: the source parser broke", name, codes[name], code)
		}
	}
	return codes
}

var rustStrConstPattern = regexp.MustCompile(`(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?const\s+([A-Z][A-Z0-9_]*)\s*:\s*&(?:'static\s+)?str\s*=\s*"([^"]*)"\s*;`)

// rustStrConsts is every `const NAME: &str = "value";` in source.
func rustStrConsts(source string) map[string]string {
	consts := map[string]string{}
	for _, match := range rustStrConstPattern.FindAllStringSubmatch(source, -1) {
		consts[match[1]] = match[2]
	}
	return consts
}

// rustBlock is the text inside the braces of the first block that follows
// marker, brace-matched.
func rustBlock(t *testing.T, source, marker string) string {
	t.Helper()
	start := strings.Index(source, marker)
	if start < 0 {
		t.Fatalf("`%s` not found in inference-core's broker adapter: was it renamed?", marker)
	}
	open := start + strings.Index(source[start:], "{")
	depth := 0
	for offset, char := range source[open:] {
		switch char {
		case '{':
			depth++
		case '}':
			depth--
			if depth == 0 {
				return source[open+1 : open+offset]
			}
		}
	}
	t.Fatalf("`%s` has no closing brace", marker)
	return ""
}

var rustMatchArmPattern = regexp.MustCompile(`\b([A-Z][A-Z0-9_]+)\s*(?:=>|\|)`)

// inferenceCoreTypedCodes is the broker code of every const that is a match
// arm of inference-core's broker_failure, by const name: the codes it turns
// into a typed refusal instead of the generic Unavailable.
func inferenceCoreTypedCodes(t *testing.T, source string) map[string]string {
	t.Helper()
	consts := rustStrConsts(source)
	body := rustBlock(t, source, "fn broker_failure(")
	typed := map[string]string{}
	for _, match := range rustMatchArmPattern.FindAllStringSubmatch(body, -1) {
		value, ok := consts[match[1]]
		if !ok {
			t.Fatalf("broker_failure matches %s, which is not a `const %s: &str`: the source parser broke", match[1], match[1])
		}
		typed[match[1]] = value
	}
	if len(typed) < 6 {
		t.Fatalf("parsed %v from broker_failure: the source parser broke", typed)
	}
	return typed
}

var (
	rustFieldPattern  = regexp.MustCompile(`^(?:pub(?:\([^)]*\))?\s+)?([a-z_][a-z0-9_]*)\s*:`)
	rustRenamePattern = regexp.MustCompile(`\brename\s*=\s*"([^"]+)"`)
)

// rustStructJSONFields is the JSON name serde gives each field of the named
// struct: its own #[serde(rename = "...")], else its name under the
// struct's rename_all (only camelCase is understood).
func rustStructJSONFields(t *testing.T, source, name string) []string {
	t.Helper()
	marker := "struct " + name + " {"
	start := strings.Index(source, marker)
	if start < 0 {
		t.Fatalf("`%s` not found in inference-core's broker adapter: was it renamed?", marker)
	}
	// The attributes above the struct, up to the previous item.
	lines := strings.Split(source[:start], "\n")
	camel := false
	for index := len(lines) - 2; index >= 0; index-- {
		line := strings.TrimSpace(lines[index])
		if !strings.HasPrefix(line, "#[") && !strings.HasPrefix(line, "///") {
			break
		}
		if strings.Contains(line, "rename_all") {
			if !strings.Contains(line, `rename_all = "camelCase"`) {
				t.Fatalf("%s uses %s; teach this test that casing", name, line)
			}
			camel = true
		}
	}
	var fields []string
	pendingRename := ""
	for _, raw := range strings.Split(rustBlock(t, source, marker), "\n") {
		line := strings.TrimSpace(raw)
		switch {
		case line == "" || strings.HasPrefix(line, "//"):
			continue
		case strings.HasPrefix(line, "#["):
			if match := rustRenamePattern.FindStringSubmatch(line); match != nil {
				pendingRename = match[1]
			}
			continue
		}
		match := rustFieldPattern.FindStringSubmatch(line)
		if match == nil {
			continue
		}
		field := match[1]
		switch {
		case pendingRename != "":
			field = pendingRename
		case camel:
			field = snakeToCamel(field)
		}
		fields = append(fields, field)
		pendingRename = ""
	}
	return fields
}

func snakeToCamel(snake string) string {
	parts := strings.Split(snake, "_")
	for index := 1; index < len(parts); index++ {
		if parts[index] != "" {
			parts[index] = strings.ToUpper(parts[index][:1]) + parts[index][1:]
		}
	}
	return strings.Join(parts, "")
}

// Every reason the broker can send, other than the generic
// subscription_broker_failed, is one inference-core types. A code renamed or
// added here without inference-core learning it would reach the chat UI as
// the generic "the model did not answer".
func TestInferenceCoreTypesEveryBrokerReason(t *testing.T) {
	source := inferenceCoreBrokerSource(t)
	typed := map[string]bool{}
	for _, code := range inferenceCoreTypedCodes(t, source) {
		typed[code] = true
	}
	for name, code := range brokerFailureCodes(t) {
		if code == string(codexsubscription.FailureBrokerFailed) {
			// The broker's own "anything else": inference-core keeps it
			// generic on purpose.
			continue
		}
		if !typed[code] {
			t.Errorf("the broker sends %s = %q and inference-core's broker_failure does not type it: inference-core would fall back to its generic Unavailable and hide the reason", name, code)
		}
	}
}

// The error fields inference-core reads are spelled as the broker writes
// them: in the unary error object, and on the stream's events.
func TestInferenceCoreReadsTheBrokerFieldNames(t *testing.T) {
	source := inferenceCoreBrokerSource(t)

	errorFields := rustStructJSONFields(t, source, "BrokerError")
	for _, want := range []string{"code", "resetsAt", "retryAfterMs"} {
		if !slices.Contains(errorFields, want) {
			t.Fatalf("inference-core's BrokerError reads %v, without %q: the parser broke or the field was renamed", errorFields, want)
		}
	}
	resetsAt := time.Now().Add(2 * time.Hour).UTC().Truncate(time.Second)
	app, _ := fakeRunnerStack(t, &apiCodexRunner{invokeErr: &codexsubscription.RateLimitError{Kind: codexsubscription.RateLimitUsage, ResetsAt: resetsAt, RetryAfter: 2 * time.Hour}}, nil)
	resp := postInfer(t, app, unaryInferPath, directInferBody, -1)
	body := readBody(t, resp)
	var envelope struct {
		Success *bool          `json:"success"`
		Error   map[string]any `json:"error"`
	}
	if err := json.Unmarshal([]byte(body), &envelope); err != nil || envelope.Success == nil || *envelope.Success || envelope.Error == nil {
		t.Fatalf("unary usage-limit body = %s (%v)", body, err)
	}
	for _, field := range errorFields {
		if _, ok := envelope.Error[field]; !ok {
			t.Errorf("inference-core reads error.%s and the broker's unary error does not carry it: %s", field, body)
		}
	}

	streamFields := rustStructJSONFields(t, source, "BrokerStreamEvent")
	for _, want := range []string{"type", "delta", "code", "resetsAt", "retryAfterMs"} {
		if !slices.Contains(streamFields, want) {
			t.Fatalf("inference-core's BrokerStreamEvent reads %v, without %q: the parser broke or the field was renamed", streamFields, want)
		}
	}
	written := map[string]bool{}
	eventType := reflect.TypeFor[codexSubscriptionStreamEvent]()
	for index := range eventType.NumField() {
		name, _, _ := strings.Cut(eventType.Field(index).Tag.Get("json"), ",")
		written[name] = true
	}
	for _, field := range streamFields {
		if !written[field] {
			t.Errorf("inference-core reads the stream event's %q and the broker's codexSubscriptionStreamEvent has no such field (it has %v)", field, written)
		}
	}
	// A usage-limit error event carries both timing fields under those names.
	encoded, err := json.Marshal(subscriptionFailureStreamEvent(codexsubscription.ClassifyFailure(&codexsubscription.RateLimitError{Kind: codexsubscription.RateLimitUsage, ResetsAt: resetsAt}, time.Now())))
	if err != nil {
		t.Fatal(err)
	}
	var event map[string]any
	if err := json.Unmarshal(encoded, &event); err != nil {
		t.Fatal(err)
	}
	for _, field := range []string{"resetsAt", "retryAfterMs"} {
		if _, ok := event[field]; !ok {
			t.Errorf("the usage-limit error event %s lacks %q", encoded, field)
		}
	}
}

// The broker's pre-flight code for an inactive connection is the one
// inference-core reads as "reconnect", on both routes.
func TestInactiveConnectionCodeIsTheOneInferenceCoreReads(t *testing.T) {
	source := inferenceCoreBrokerSource(t)
	typed := inferenceCoreTypedCodes(t, source)
	want, ok := typed["CONNECTION_INACTIVE"]
	if !ok {
		t.Fatalf("inference-core's broker_failure no longer types CONNECTION_INACTIVE (it types %v): the broker still sends it for an inactive connection", typed)
	}
	app, repo := fakeRunnerStack(t, &apiCodexRunner{}, nil)
	connection, err := repo.GetConnection(t.Context(), directConnectionID)
	if err != nil {
		t.Fatal(err)
	}
	connection.Status = "disconnected"
	if _, err := repo.UpsertConnection(t.Context(), connection); err != nil {
		t.Fatal(err)
	}
	for _, path := range []string{unaryInferPath, streamInferPath} {
		resp := postInfer(t, app, path, directInferBody, -1)
		body := readBody(t, resp)
		if resp.StatusCode != http.StatusConflict || decodeAPIError(t, body).Error.Code != want {
			t.Fatalf("%s: status = %d body = %s, want 409 %s", path, resp.StatusCode, body, want)
		}
	}
}
