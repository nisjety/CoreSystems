package main

import (
	"encoding/json"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"testing"
	"time"

	natsserver "github.com/nats-io/nats-server/v2/server"
	"github.com/nats-io/nats.go"
)

// Contract tests for the Model Plane's NATS topology.
//
// They apply the REAL rules: the real manifest (streamConfigs, consumerBindings
// through provisionTopology, exactly what the deployment runs), the real broker
// configuration (deploy/nats.conf, with its users and publish/subscribe grants),
// the real subjects the services publish (publishers_registry_test.go, tied to
// the source by needle text) and the real wire fixtures session-core and
// audit-core share. Nothing here is a stub of the broker or of a rule.
//
// What they exist to prevent: on 2026-10-03 the live stack held 4,992 audit rows
// that had run out of delivery attempts because the broker had no stream for
// their subject. No test connected the publisher's subject to any stream.

const (
	modelPlaneRoot   = "../../.."
	controlPlaneRoot = "../../../../Control Plane"
	probePassword    = "0123456789abcdef0123456789abcdef"
)

var brokerPasswordVariable = regexp.MustCompile(`\$([A-Z][A-Z0-9_]*PASSWORD)\b`)

// startModelBroker runs an in-process NATS server with JetStream from the real
// deploy/nats.conf. Compose wraps the broker's copy of each password in quotes
// so any generated value parses as a string; the test does the same.
func startModelBroker(t *testing.T) *natsserver.Server {
	t.Helper()
	confPath := filepath.Join(modelPlaneRoot, "deploy", "nats.conf")
	contents, err := os.ReadFile(confPath)
	if err != nil {
		t.Fatalf("read broker config: %v", err)
	}
	names := map[string]struct{}{}
	for _, match := range brokerPasswordVariable.FindAllStringSubmatch(string(contents), -1) {
		names[match[1]] = struct{}{}
	}
	if len(names) == 0 {
		t.Fatal("deploy/nats.conf names no password variables: the test would not be using the real users")
	}
	for name := range names {
		t.Setenv(name, strconv.Quote(probePassword))
	}
	options, err := natsserver.ProcessConfigFile(confPath)
	if err != nil {
		t.Fatalf("process deploy/nats.conf: %v", err)
	}
	if !options.JetStream {
		t.Fatal("deploy/nats.conf does not enable JetStream")
	}
	options.Port = -1
	options.HTTPPort = -1
	options.StoreDir = t.TempDir()
	options.NoLog = true
	options.NoSigs = true
	instance, err := natsserver.NewServer(options)
	if err != nil {
		t.Fatalf("build broker: %v", err)
	}
	go instance.Start()
	if !instance.ReadyForConnections(15 * time.Second) {
		t.Fatal("Model Plane broker did not start")
	}
	t.Cleanup(func() { instance.Shutdown(); instance.WaitForShutdown() })
	return instance
}

// connectAs opens a connection as a broker principal, with the custom inbox
// prefix its service sets, and returns it with the channel its asynchronous
// errors (permission violations among them) arrive on.
func connectAs(t *testing.T, instance *natsserver.Server, user, inbox string) (*nats.Conn, <-chan error) {
	t.Helper()
	asyncErrors := make(chan error, 16)
	connection, err := nats.Connect(instance.ClientURL(),
		nats.UserInfo(user, probePassword),
		nats.CustomInboxPrefix(inbox),
		nats.Timeout(3*time.Second),
		nats.ErrorHandler(func(_ *nats.Conn, _ *nats.Subscription, err error) {
			select {
			case asyncErrors <- err:
			default:
			}
		}),
	)
	if err != nil {
		t.Fatalf("connect as %s: %v", user, err)
	}
	t.Cleanup(connection.Close)
	return connection, asyncErrors
}

// provisionModelTopology runs the deployment's provisioner against the broker
// as the same principal compose runs it as.
func provisionModelTopology(t *testing.T, instance *natsserver.Server) {
	t.Helper()
	connection, _ := connectAs(t, instance, "observability-provisioner-model", "_INBOX.PROVISIONER_MODEL")
	// A generous API wait: stream creation on a loaded machine must not read as a broken manifest.
	js, err := connection.JetStream(nats.MaxWait(30 * time.Second))
	if err != nil {
		t.Fatalf("provisioner JetStream context: %v", err)
	}
	if err := provisionTopology(js); err != nil {
		t.Fatalf("the manifest does not converge on a fresh broker: %v", err)
	}
	// A second run must be a no-op: the provisioner runs on every deploy.
	if err := provisionTopology(js); err != nil {
		t.Fatalf("re-running the manifest is not idempotent: %v", err)
	}
}

// concreteSubject turns the registry's tenant placeholder token (x) into a real
// token, so the probe is published on a subject shaped like production's.
func concreteSubject(subject string) string {
	tokens := strings.Split(subject, ".")
	for index, token := range tokens {
		if token == "x" {
			tokens[index] = "contract-tenant"
		}
	}
	return strings.Join(tokens, ".")
}

// ---- 1. The audit observability topology exists after provisioning. ----

func TestManifestProvisionsTheAuditObservabilityStreamAndDurables(t *testing.T) {
	instance := startModelBroker(t)
	provisionModelTopology(t, instance)
	connection, _ := connectAs(t, instance, "observability-provisioner-model", "_INBOX.PROVISIONER_MODEL")
	js, err := connection.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	info, err := js.StreamInfo(auditObservabilityStreamName)
	if err != nil {
		t.Fatalf("stream %s does not exist after provisioning: %v", auditObservabilityStreamName, err)
	}
	if got, want := strings.Join(info.Config.Subjects, " "), strings.Join(auditObservabilityStream().Subjects, " "); got != want {
		t.Fatalf("stream subjects = %q, want %q", got, want)
	}
	for _, durable := range []string{"audit-core-model-v3-audit", "audit-core-model-v3-usage"} {
		if _, err := js.ConsumerInfo(auditObservabilityStreamName, durable); err != nil {
			t.Fatalf("audit-core durable %s is missing after provisioning: %v", durable, err)
		}
	}
}

// ---- 2. Every JetStream publisher's real subject gets a PubAck. ----

// This is the test the live failure would have failed: session-core publishes
// its governed tool actions with a JetStream publish and the broker had no
// stream for the subject. Each row publishes as its own principal, through the
// real manifest and the real broker grants, and must be acknowledged by the
// stream the registry names.
func TestEveryJetStreamPublisherSubjectGetsAPubAck(t *testing.T) {
	instance := startModelBroker(t)
	provisionModelTopology(t, instance)
	checked := 0
	for _, row := range publisherRegistry() {
		if row.transport != jetStreamAck {
			continue
		}
		checked++
		row := row
		t.Run(row.service+" "+row.subject, func(t *testing.T) {
			connection, _ := connectAs(t, instance, row.natsUser, row.inbox)
			js, err := connection.JetStream(nats.MaxWait(3 * time.Second))
			if err != nil {
				t.Fatal(err)
			}
			subject := concreteSubject(row.subject)
			message := nats.NewMsg(subject)
			message.Data = []byte(`{"contract":"publisher-probe"}`)
			message.Header.Set(nats.MsgIdHdr, "contract-probe:"+row.service+":"+subject)
			ack, err := js.PublishMsg(message)
			if err != nil {
				t.Fatalf("%s publishing %q as %s got no acknowledgement: %v\n"+
					"The broker has no stream that captures this subject, or %s may not publish it. "+
					"Add the stream to streamConfigs() (the provisioner owns it) or fix deploy/nats.conf.",
					row.service, subject, row.natsUser, err, row.natsUser)
			}
			if ack.Stream != row.stream {
				t.Fatalf("%q was captured by stream %q, the registry says %q", subject, ack.Stream, row.stream)
			}
		})
	}
	if checked == 0 {
		t.Fatal("the registry lists no JetStream publisher: the test would check nothing")
	}
}

// ---- 3. audit-core's durable receives what session-core publishes. ----

type wireFixture struct {
	Subject string          `json:"subject"`
	Body    json.RawMessage `json:"body"`
}

func loadWireFixtures(t *testing.T) map[string]wireFixture {
	t.Helper()
	dir := filepath.Join(modelPlaneRoot, "rust", "services", "session-core", "tests", "fixtures", "audit-core-wire")
	entries, err := os.ReadDir(dir)
	if err != nil {
		t.Fatalf("read wire fixtures: %v", err)
	}
	fixtures := map[string]wireFixture{}
	for _, entry := range entries {
		raw, err := os.ReadFile(filepath.Join(dir, entry.Name()))
		if err != nil {
			t.Fatal(err)
		}
		var fixture wireFixture
		if err := json.Unmarshal(raw, &fixture); err != nil {
			t.Fatalf("fixture %s: %v", entry.Name(), err)
		}
		fixtures[entry.Name()] = fixture
	}
	if len(fixtures) < 2 {
		t.Fatalf("expected the final and reserved wire fixtures, found %d", len(fixtures))
	}
	return fixtures
}

// The whole path, with the real grants on both ends: session-core publishes the
// fixture bodies (the bytes its own builder produces, pinned by its tests) on the
// subjects its own code picks; audit-core's principal binds its durable exactly
// as audit-core does (queue subscribe + nats.Bind + manual ack) and must receive
// every one of them, on the subject the body's event names.
func TestAuditCoreDurableReceivesWhatSessionCorePublishes(t *testing.T) {
	instance := startModelBroker(t)
	provisionModelTopology(t, instance)

	auditCore, _ := connectAs(t, instance, "audit-core-model", "_INBOX.AUDIT_MODEL")
	auditJS, err := auditCore.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	type delivery struct {
		subject string
		body    []byte
	}
	received := make(chan delivery, 8)
	const durable = "audit-core-model-v3-audit"
	if _, err := auditJS.QueueSubscribe("verevon.audit.v2.model.>", durable, func(message *nats.Msg) {
		received <- delivery{subject: message.Subject, body: append([]byte(nil), message.Data...)}
		_ = message.Ack()
	}, nats.Bind(auditObservabilityStreamName, durable), nats.ManualAck(), nats.AckExplicit()); err != nil {
		t.Fatalf("audit-core cannot bind its durable on the Model Plane broker: %v", err)
	}

	sessionCore, _ := connectAs(t, instance, sessionCoreUser, sessionCoreBox)
	sessionJS, err := sessionCore.JetStream(nats.MaxWait(3 * time.Second))
	if err != nil {
		t.Fatal(err)
	}
	want := map[string]wireFixture{}
	published := 0
	for name, fixture := range loadWireFixtures(t) {
		message := nats.NewMsg(fixture.Subject)
		message.Data = fixture.Body
		message.Header.Set(nats.MsgIdHdr, "contract-fixture:"+name)
		if _, err := sessionJS.PublishMsg(message); err != nil {
			t.Fatalf("session-core publishing fixture %s on %q: %v", name, fixture.Subject, err)
		}
		want[fixture.Subject] = fixture
		published++
	}
	for len(want) > 0 {
		select {
		case got := <-received:
			fixture, ok := want[got.subject]
			if !ok {
				t.Fatalf("audit-core received an unexpected subject %q", got.subject)
			}
			var expected, actual map[string]any
			if err := json.Unmarshal(fixture.Body, &expected); err != nil {
				t.Fatal(err)
			}
			if err := json.Unmarshal(got.body, &actual); err != nil {
				t.Fatal(err)
			}
			if fmt.Sprint(expected) != fmt.Sprint(actual) {
				t.Fatalf("body altered in transit on %q", got.subject)
			}
			delete(want, got.subject)
		case <-time.After(5 * time.Second):
			t.Fatalf("audit-core's durable did not deliver %d of the published events: %v", len(want), subjectsOf(want))
		}
	}

	// Delivery is not enough. audit-core acknowledges with a publish to
	// $JS.ACK.<stream>.<durable>.>, and a principal without that grant has its
	// acknowledgement dropped by the broker with no error the handler can see:
	// every event would then be redelivered up to MaxDeliver times and the
	// durable's backlog would stay stuck. Require the broker to have accepted
	// every acknowledgement (read through the provisioner, which may read state).
	provisioner, _ := connectAs(t, instance, "observability-provisioner-model", "_INBOX.PROVISIONER_MODEL")
	provisionerJS, err := provisioner.JetStream(nats.MaxWait(5 * time.Second))
	if err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(5 * time.Second)
	for {
		info, err := provisionerJS.ConsumerInfo(auditObservabilityStreamName, durable)
		if err != nil {
			t.Fatalf("read the durable's state: %v", err)
		}
		if info.NumAckPending == 0 && info.AckFloor.Consumer == uint64(published) {
			break
		}
		if time.Now().After(deadline) {
			t.Fatalf("the broker did not accept audit-core's acknowledgements (%d pending, ack floor %d of %d delivered): "+
				"audit-core's principal needs publish on $JS.ACK.%s.%s.> in deploy/nats.conf",
				info.NumAckPending, info.AckFloor.Consumer, published, auditObservabilityStreamName, durable)
		}
		time.Sleep(100 * time.Millisecond)
	}
}

func subjectsOf(rows map[string]wireFixture) []string {
	subjects := make([]string, 0, len(rows))
	for subject := range rows {
		subjects = append(subjects, subject)
	}
	sort.Strings(subjects)
	return subjects
}

// ---- 4. The registry is true: each row's text is in the real source. ----

func TestRegistryRowsPointAtRealSource(t *testing.T) {
	seen := map[string]string{}
	for _, row := range publisherRegistry() {
		contents, err := os.ReadFile(filepath.Join(modelPlaneRoot, filepath.FromSlash(row.source)))
		if err != nil {
			t.Errorf("%s %s: source %s is unreadable: %v", row.service, row.subject, row.source, err)
			continue
		}
		if !strings.Contains(string(contents), row.needle) {
			t.Errorf("%s %s: %s no longer contains %q; the publisher changed, update publishers_registry_test.go",
				row.service, row.subject, row.source, row.needle)
		}
		key := row.service + "|" + row.subject
		if previous, duplicate := seen[key]; duplicate {
			t.Errorf("duplicate registry row %s (also %s)", key, previous)
		}
		seen[key] = row.source
	}
}

// ---- 5. Nothing publishes to JetStream without being registered. ----

func modelPlaneSources(t *testing.T, extensions ...string) []string {
	t.Helper()
	var files []string
	for _, tree := range []string{"rust", "go"} {
		root := filepath.Join(modelPlaneRoot, tree)
		err := filepath.WalkDir(root, func(path string, entry fs.DirEntry, err error) error {
			if err != nil {
				return err
			}
			slashed := filepath.ToSlash(path)
			if entry.IsDir() {
				switch entry.Name() {
				case "target", "node_modules", ".git", "tests", "gen":
					return filepath.SkipDir
				}
				return nil
			}
			name := entry.Name()
			if strings.HasSuffix(name, "_test.go") || strings.HasSuffix(name, "tests.rs") || strings.HasSuffix(name, "_test.rs") {
				return nil
			}
			for _, extension := range extensions {
				if strings.HasSuffix(name, extension) {
					files = append(files, slashed)
					break
				}
			}
			return nil
		})
		if err != nil {
			t.Fatalf("walk %s: %v", root, err)
		}
	}
	sort.Strings(files)
	return files
}

func relativeToModelPlane(path string) string {
	return strings.TrimPrefix(filepath.ToSlash(path), strings.TrimSuffix(filepath.ToSlash(modelPlaneRoot), "/")+"/")
}

// The call forms of a JetStream publish that waits for (or can wait for) a PubAck:
// async-nats' publish_with_headers / publish_with_ack and its Context::publish on a
// variable named js or jetstream, and nats.go's JetStreamContext publishers. A
// publisher written in another form is not seen here (a new NATS client module is
// still caught by TestEveryNATSClientModuleIsAccountedFor).
var jetStreamPublishCall = regexp.MustCompile(
	`publish_with_headers\(|\.publish_with_ack\(|\b(js|jetstream)\.publish\(|\bjs\.(Publish|PublishMsg|PublishAsync|PublishMsgAsync)\(|\bjetstream\.(Publish|PublishMsg)\(`)

func TestEveryJetStreamPublishCallSiteIsRegistered(t *testing.T) {
	registered := map[string]bool{}
	for _, row := range publisherRegistry() {
		if row.transport == jetStreamAck {
			registered[row.source] = true
		}
	}
	found := map[string]bool{}
	for _, path := range modelPlaneSources(t, ".rs", ".go") {
		contents, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		if jetStreamPublishCall.Match(contents) {
			found[relativeToModelPlane(path)] = true
		}
	}
	for path := range found {
		if !registered[path] {
			t.Errorf("%s publishes to JetStream and no jetstream-ack row in publishers_registry_test.go names it: "+
				"register its subjects and give each one a stream in streamConfigs()", path)
		}
	}
	for path := range registered {
		if !found[path] {
			t.Errorf("registry says %s publishes to JetStream, but no JetStream publish call is left in it", path)
		}
	}
}

func TestEveryNATSClientModuleIsAccountedFor(t *testing.T) {
	expected := natsClientModules()
	found := map[string]bool{}
	for _, pattern := range []string{
		filepath.Join(modelPlaneRoot, "go", "services", "*", "go.mod"),
		filepath.Join(modelPlaneRoot, "go", "pkg", "*", "go.mod"),
		filepath.Join(modelPlaneRoot, "rust", "services", "*", "Cargo.toml"),
		filepath.Join(modelPlaneRoot, "rust", "crates", "*", "Cargo.toml"),
	} {
		matches, err := filepath.Glob(pattern)
		if err != nil {
			t.Fatal(err)
		}
		for _, manifest := range matches {
			contents, err := os.ReadFile(manifest)
			if err != nil {
				t.Fatal(err)
			}
			if strings.Contains(string(contents), "nats-io/nats.go") || strings.Contains(string(contents), "async-nats") {
				found[relativeToModelPlane(filepath.Dir(manifest))] = true
			}
		}
	}
	for module := range found {
		if _, ok := expected[module]; !ok {
			t.Errorf("%s links a NATS client and natsClientModules() does not list it: decide what it publishes, "+
				"register those subjects and give each a stream", module)
		}
	}
	for module := range expected {
		if !found[module] {
			t.Errorf("natsClientModules() lists %s but it no longer links a NATS client", module)
		}
	}
	// execution-core and inference-core must stay off the bus: they reach NATS
	// consumers only through session-core's gRPC (RecordOrchestrationEvent).
	for _, module := range []string{"execution-core", "inference-core"} {
		manifest := filepath.Join(modelPlaneRoot, "rust", "services", module, "Cargo.toml")
		contents, err := os.ReadFile(manifest)
		if err != nil {
			t.Fatal(err)
		}
		if strings.Contains(string(contents), "nats") {
			t.Errorf("%s links a NATS client; register what it publishes in publishers_registry_test.go", module)
		}
	}
}

// ---- 6. The subject catalogue is fully accounted for. ----

var subjectBuilderHeader = regexp.MustCompile(`pub fn (\w+_subject)\(([^)]*)\) -> String`)
var formatLiteral = regexp.MustCompile(`format!\("([^"]*)"`)
var placeholder = regexp.MustCompile(`\{[A-Za-z_]+\}`)

// subjectBuilders returns every mp-events subject builder with its format string.
func subjectBuilders(t *testing.T) map[string]string {
	t.Helper()
	path := filepath.Join(modelPlaneRoot, "rust", "crates", "mp-events", "src", "subjects.rs")
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	source := strings.ReplaceAll(string(raw), "\r\n", "\n")
	// Builders live above the test module; do not read format strings from tests.
	if cut := strings.Index(source, "#[cfg(test)]"); cut >= 0 {
		source = source[:cut]
	}
	builders := map[string]string{}
	for _, match := range subjectBuilderHeader.FindAllStringSubmatchIndex(source, -1) {
		name := source[match[2]:match[3]]
		body := source[match[1]:]
		if end := strings.Index(body, "\n}"); end >= 0 {
			body = body[:end]
		}
		literal := formatLiteral.FindStringSubmatch(body)
		if literal == nil {
			// A transform such as translate_legacy_subject maps one subject to
			// another and does not build one from tokens: not a builder.
			continue
		}
		builders[name] = literal[1]
	}
	if len(builders) < 5 {
		t.Fatalf("found only %d subject builders in subjects.rs: the parser is broken", len(builders))
	}
	return builders
}

func evaluateBuilder(format string, args []string) string {
	subject := strings.ReplaceAll(format, "{PREFIX}", "mp.v1")
	for _, arg := range args {
		loc := placeholder.FindStringIndex(subject)
		if loc == nil {
			break
		}
		subject = subject[:loc[0]] + arg + subject[loc[1]:]
	}
	return subject
}

func TestRegisteredSubjectsAreWhatTheRealBuildersProduce(t *testing.T) {
	builders := subjectBuilders(t)
	for _, row := range publisherRegistry() {
		if row.builder == "" {
			continue
		}
		format, ok := builders[row.builder]
		if !ok {
			t.Errorf("%s %s: mp-events has no builder %s", row.service, row.subject, row.builder)
			continue
		}
		if got := evaluateBuilder(format, row.args); got != row.subject {
			t.Errorf("%s: builder %s%v produces %q, the registry says %q", row.service, row.builder, row.args, got, row.subject)
		}
	}
}

func TestEverySubjectBuilderIsEitherRegisteredOrDeclaredUnused(t *testing.T) {
	builders := subjectBuilders(t)
	registered := map[string]bool{}
	for _, row := range publisherRegistry() {
		if row.builder != "" {
			registered[row.builder] = true
		}
	}
	unused := unusedSubjectBuilders()
	sources := modelPlaneSources(t, ".rs")
	contents := make(map[string]string, len(sources))
	for _, path := range sources {
		if strings.HasSuffix(filepath.ToSlash(path), "mp-events/src/subjects.rs") {
			continue
		}
		raw, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		contents[path] = string(raw)
	}
	names := make([]string, 0, len(builders))
	for name := range builders {
		names = append(names, name)
	}
	sort.Strings(names)
	for _, name := range names {
		call := regexp.MustCompile(`\b` + name + `\(`)
		used := false
		for _, text := range contents {
			if call.MatchString(text) {
				used = true
				break
			}
		}
		_, declaredUnused := unused[name]
		switch {
		case used && !registered[name] && !declaredUnused:
			t.Errorf("subject builder %s has callers and no registry row: it publishes somewhere the contract does not know", name)
		case used && declaredUnused:
			t.Errorf("subject builder %s is listed as unused but now has callers: register it", name)
		case !used && registered[name]:
			t.Errorf("subject builder %s is registered but nothing calls it any more", name)
		case !used && !declaredUnused:
			t.Errorf("subject builder %s has no callers and is not listed in unusedSubjectBuilders()", name)
		}
	}
	for name := range unused {
		if _, exists := builders[name]; !exists {
			t.Errorf("unusedSubjectBuilders() lists %s, which mp-events no longer defines", name)
		}
	}
}

// ---- 7. Every publisher may publish what the registry says it publishes. ----

func TestPublisherPrincipalsMayPublishTheirSubjects(t *testing.T) {
	instance := startModelBroker(t)
	provisionModelTopology(t, instance)
	for _, row := range publisherRegistry() {
		if row.transport != coreNATS {
			continue
		}
		row := row
		t.Run(row.service+" "+row.subject, func(t *testing.T) {
			connection, asyncErrors := connectAs(t, instance, row.natsUser, row.inbox)
			subject := concreteSubject(row.subject)
			if err := connection.Publish(subject, []byte(`{"contract":"publisher-probe"}`)); err != nil {
				t.Fatalf("publish: %v", err)
			}
			_ = connection.FlushTimeout(2 * time.Second)
			var denied error
			select {
			case denied = <-asyncErrors:
			case <-time.After(750 * time.Millisecond):
			}
			isPermissionError := denied != nil && strings.Contains(strings.ToLower(denied.Error()), "permission")
			switch {
			case row.gap == "" && isPermissionError:
				t.Fatalf("%s may not publish %q as %s: %v.\n"+
					"A core-NATS publish gives the sender no error, so this would be silent in production. "+
					"Add the grant to deploy/nats.conf.", row.service, subject, row.natsUser, denied)
			case row.gap != "" && !isPermissionError:
				t.Fatalf("registry marks %s %q as a known gap (%s) but the broker now accepts it: "+
					"remove the gap from publishers_registry_test.go", row.service, subject, row.gap)
			case row.gap != "":
				t.Logf("KNOWN GAP (not fixed here): %s publishing %q: %s", row.service, subject, row.gap)
			}
		})
	}
}

// ---- 8. This provisioner and audit-core's must not fight over the stream. ----

var (
	auditCoreConstant = func(name string) *regexp.Regexp {
		return regexp.MustCompile(`\b` + name + `\s*=\s*"([^"]+)"`)
	}
	sprintfOf = func(function string) *regexp.Regexp {
		return regexp.MustCompile(`func ` + function + `\([^)]*\) string \{\s*return fmt\.Sprintf\("([^"]+)"`)
	}
)

// audit-core's provisioner (Control Plane, optional extra-plane run) converges
// this same stream with UpdateStream, which replaces the subject list wholesale.
// If it ever logs in successfully, any difference between its idea of the stream
// and this manifest's would flip the stream back and forth on every deploy.
func TestObservabilityTopologyMatchesAuditCoreProvisioner(t *testing.T) {
	path := filepath.Join(controlPlaneRoot, "audit-core", "internal", "provisioner", "provisioner.go")
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read audit-core's provisioner: %v", err)
	}
	source := strings.ReplaceAll(string(raw), "\r\n", "\n")

	constant := func(name string) string {
		match := auditCoreConstant(name).FindStringSubmatch(source)
		if match == nil {
			t.Fatalf("audit-core's provisioner no longer defines %s: update this contract test", name)
		}
		return match[1]
	}
	function := func(name string) string {
		match := sprintfOf(name).FindStringSubmatch(source)
		if match == nil {
			t.Fatalf("audit-core's provisioner function %s changed shape: update this contract test", name)
		}
		return match[1]
	}
	sprintf := func(format string, args ...string) string {
		for _, arg := range args {
			format = strings.Replace(format, "%s", arg, 1)
		}
		return format
	}

	plane := auditObservabilityPlane
	if got, want := constant("StreamName"), auditObservabilityStream().Name; got != want {
		t.Errorf("stream name: audit-core %q, manifest %q", got, want)
	}
	if got, want := constant("DLQSubject"), auditCoreDLQSubject; got != want {
		t.Errorf("DLQ subject: audit-core %q, manifest %q", got, want)
	}

	// The subject list audit-core wants, in its order, evaluated for this plane.
	block := regexp.MustCompile(`wantedSubjects := \[\]string\{\n((?:\s+[^\n]+,\n)+)\s*\}`).FindStringSubmatch(source)
	if block == nil {
		t.Fatal("audit-core's wantedSubjects list changed shape: update this contract test")
	}
	var theirs []string
	for _, line := range strings.Split(strings.TrimSpace(block[1]), "\n") {
		line = strings.TrimSuffix(strings.TrimSpace(line), ",")
		switch {
		case strings.HasPrefix(line, "LegacyPlaneSubject("):
			kind := strings.Split(line, `"`)[1]
			theirs = append(theirs, sprintf(function("LegacyPlaneSubject"), kind, plane))
		case strings.HasPrefix(line, "PlaneSubject("):
			kind := strings.Split(line, `"`)[1]
			theirs = append(theirs, sprintf(function("PlaneSubject"), kind, plane))
		case line == "DLQSubject":
			theirs = append(theirs, constant("DLQSubject"))
		default:
			t.Fatalf("audit-core's wantedSubjects has an entry this test does not understand: %q", line)
		}
	}
	if got, want := strings.Join(theirs, " "), strings.Join(auditObservabilityStream().Subjects, " "); got != want {
		t.Errorf("stream subjects: audit-core %q, manifest %q", got, want)
	}

	stream := auditObservabilityStream()
	for _, expectation := range []struct {
		name string
		want string
	}{
		{"MaxAge: 30 * 24 * time.Hour", fmt.Sprintf("MaxAge: %s", "30 * 24 * time.Hour")},
		{"MaxMsgs: 2_000_000", "MaxMsgs: 2_000_000"},
		{"Retention: nats.LimitsPolicy", "Retention: nats.LimitsPolicy"},
		{"Discard: nats.DiscardOld", "Discard: nats.DiscardOld"},
		{"Storage: nats.FileStorage", "Storage: nats.FileStorage"},
	} {
		if !regexp.MustCompile(strings.ReplaceAll(regexp.QuoteMeta(expectation.name), " ", `\s+`)).MatchString(source) {
			t.Errorf("audit-core's provisioner no longer sets %q: compare with the manifest", expectation.name)
		}
	}
	if stream.MaxAge != 30*24*time.Hour || stream.MaxMsgs != 2_000_000 ||
		stream.Retention != nats.LimitsPolicy || stream.Discard != nats.DiscardOld || stream.Storage != nats.FileStorage {
		t.Errorf("manifest stream no longer matches audit-core's limits: %+v", stream)
	}

	// The two durables, evaluated for this plane.
	for _, binding := range auditObservabilityConsumers() {
		kind := "audit"
		if strings.HasSuffix(binding.config.Durable, "-usage") {
			kind = "usage"
		}
		want := map[string]string{
			"durable":        sprintf(function("ConsumerName"), plane, kind),
			"deliverSubject": sprintf(function("DeliverySubject"), plane, kind),
			"filter":         sprintf(function("PlaneSubject"), kind, plane),
		}
		got := map[string]string{
			"durable":        binding.config.Durable,
			"deliverSubject": binding.config.DeliverSubject,
			"filter":         binding.config.FilterSubject,
		}
		for field := range want {
			if got[field] != want[field] {
				t.Errorf("%s consumer %s: audit-core %q, manifest %q", kind, field, want[field], got[field])
			}
		}
		if binding.config.DeliverGroup != binding.config.Durable {
			t.Errorf("%s consumer deliver group %q must equal its durable name (audit-core queue-subscribes with it)",
				kind, binding.config.DeliverGroup)
		}
		if binding.config.MaxDeliver != 5 || binding.config.AckWait != 30*time.Second ||
			binding.config.AckPolicy != nats.AckExplicitPolicy || binding.config.DeliverPolicy != nats.DeliverAllPolicy {
			t.Errorf("%s consumer limits drifted from audit-core's: %+v", kind, binding.config)
		}
	}
}

// ---- 9. A failed provisioner stops the stack; it cannot be skipped. ----

func composeServiceBlock(t *testing.T, compose, service string) string {
	t.Helper()
	lines := strings.Split(strings.ReplaceAll(compose, "\r\n", "\n"), "\n")
	header := "  " + service + ":"
	var block []string
	inside := false
	for _, line := range lines {
		if line == header {
			inside = true
			continue
		}
		if inside && len(line) > 2 && line[0] == ' ' && line[1] == ' ' && line[2] != ' ' && line[2] != '#' {
			break
		}
		if inside && len(line) > 0 && line[0] != ' ' && line[0] != '#' {
			break
		}
		if inside {
			block = append(block, line)
		}
	}
	if len(block) == 0 {
		t.Fatalf("compose has no service %q", service)
	}
	return strings.Join(block, "\n")
}

func TestComposeGatesSessionCoreOnTheProvisionerAndSharesTheBrokersCredential(t *testing.T) {
	raw, err := os.ReadFile(filepath.Join(modelPlaneRoot, "deploy", "docker-compose.yml"))
	if err != nil {
		t.Fatal(err)
	}
	compose := string(raw)

	session := composeServiceBlock(t, compose, "session-core")
	dependsOn := regexp.MustCompile(`nats-provisioner:\s*\n\s+condition: service_completed_successfully`)
	if !dependsOn.MatchString(session) {
		t.Error("session-core must depend on nats-provisioner with condition service_completed_successfully: " +
			"the stream has to exist before the first outbox publish, and a failed provisioner must stop the stack")
	}

	provisionerBlock := composeServiceBlock(t, compose, "nats-provisioner")
	if !strings.Contains(provisionerBlock, `restart: "no"`) {
		t.Error("nats-provisioner must not restart silently: a failure has to surface as a failed dependency")
	}
	if !strings.Contains(provisionerBlock, "NATS_USER: observability-provisioner-model") {
		t.Error("nats-provisioner must run as observability-provisioner-model, the principal deploy/nats.conf lets administer streams")
	}
	if !strings.Contains(provisionerBlock, "${MODEL_NATS_PROVISIONER_PASSWORD") {
		t.Error("nats-provisioner must read its password from MODEL_NATS_PROVISIONER_PASSWORD")
	}
	broker := composeServiceBlock(t, compose, "nats")
	if !strings.Contains(broker, "MODEL_NATS_PROVISIONER_PASSWORD:") {
		t.Error("the broker must receive MODEL_NATS_PROVISIONER_PASSWORD: the provisioner and the broker " +
			"must read one variable so their copies cannot drift apart")
	}
}
