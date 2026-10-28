#!/usr/bin/env bash
# Contract test for the Application Plane's audit-core stream (plan row WS01-S4).
#
# Mirrors apps/Model Plane/go/services/nats-provisioner/manifest_contract_test.go
# for the application broker: audit-core's provisioner (Control Plane) and this
# plane's two provisioning scripts both converge VEREVON_CONTROL_OBSERVABILITY, so
# any difference makes each undo the other, and any overlapping subject in another
# stream (the old VEREVON_AUDIT, verevon.audit.>) makes the broker refuse the
# stream outright. Section 4 also pins the runtime streams and the shared durables that
# audit-core's ProvisionApplicationRuntime converges (D1-803). Static: reads files only,
# needs no broker.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
export ROOT_DIR

exec python3 -I - <<'PY'
import os
import re
import sys

root = os.environ["ROOT_DIR"]
failures = []


def fail(message):
    failures.append(message)


def read(*parts):
    with open(os.path.join(root, *parts), "rb") as handle:
        return handle.read().decode().replace("\r\n", "\n")


PLANE = "application"
provisioner = read("..", "Control Plane", "audit-core", "internal", "provisioner", "provisioner.go")
nats_conf = read("nats.conf")
scripts = {
    "scripts/jetstream-provision-entrypoint.sh": read("scripts", "jetstream-provision-entrypoint.sh"),
    "scripts/provision-jetstream.sh": read("scripts", "provision-jetstream.sh"),
}


def const(name):
    match = re.search(r"\b" + name + r'\s*=\s*"([^"]+)"', provisioner)
    if not match:
        fail("audit-core's provisioner no longer defines %s: update this test" % name)
        return ""
    return match.group(1)


def sprintf_of(function):
    match = re.search(r"func " + function + r'\([^)]*\) string \{\s*return fmt\.Sprintf\("([^"]+)"', provisioner)
    if not match:
        fail("audit-core's provisioner function %s changed shape: update this test" % function)
        return ""
    return match.group(1)


def sprintf(fmt, *args):
    for arg in args:
        fmt = fmt.replace("%s", arg, 1)
    return fmt


stream_name = const("StreamName")
dlq = const("DLQSubject")

# The subject list audit-core wants, in its order, evaluated for this plane.
block = re.search(r"wantedSubjects := \[\]string\{\n((?:\s+[^\n]+,\n)+)\s*\}", provisioner)
wanted_subjects = []
if not block:
    fail("audit-core's wantedSubjects list changed shape: update this test")
else:
    for line in block.group(1).strip().split("\n"):
        line = line.strip().rstrip(",")
        if line.startswith("LegacyPlaneSubject("):
            wanted_subjects.append(sprintf(sprintf_of("LegacyPlaneSubject"), line.split('"')[1], PLANE))
        elif line.startswith("PlaneSubject("):
            wanted_subjects.append(sprintf(sprintf_of("PlaneSubject"), line.split('"')[1], PLANE))
        elif line == "DLQSubject":
            wanted_subjects.append(dlq)
        else:
            fail("audit-core's wantedSubjects has an entry this test does not understand: %r" % line)

for needle in ("MaxAge:    30 * 24 * time.Hour", "MaxMsgs:   2_000_000", "AckWait:        30 * time.Second", "MaxDeliver:     5"):
    if needle not in provisioner:
        fail("audit-core's provisioner no longer sets %r: compare with the scripts" % needle)

consumers = []
for kind in ("audit", "usage"):
    durable = sprintf(sprintf_of("ConsumerName"), PLANE, kind)
    consumers.append({
        "kind": kind,
        "durable": durable,
        "deliver": sprintf(sprintf_of("DeliverySubject"), PLANE, kind),
        "filter": sprintf(sprintf_of("PlaneSubject"), kind, PLANE),
    })


def subject_matches(pattern, subject):
    """NATS wildcard match of a concrete-or-wildcard subject against a pattern."""
    p, s = pattern.split("."), subject.split(".")
    for index, token in enumerate(p):
        if token == ">":
            return len(s) > index
        if index >= len(s):
            return False
        if token != "*" and s[index] not in (token, "*", ">"):
            return False
    return len(p) == len(s)


def subjects_overlap(a, b):
    """Two NATS subject patterns overlap when some subject matches both."""
    x, y = a.split("."), b.split(".")
    for index in range(max(len(x), len(y))):
        tx = x[index] if index < len(x) else None
        ty = y[index] if index < len(y) else None
        if tx == ">" or ty == ">":
            return tx is not None and ty is not None
        if tx is None or ty is None:
            return False
        if tx != "*" and ty != "*" and tx != ty:
            return False
    return True


# ---- 1. Both provisioning scripts create exactly what audit-core converges. ----
for name, text in scripts.items():
    flat = re.sub(r"\\\n\s*", " ", text)
    if "AUDIT_STREAM=" + stream_name not in flat and "audit_stream=" + stream_name not in flat:
        fail("%s does not provision %s" % (name, stream_name))
    subjects = re.search(r"--subjects='([^']*" + re.escape(PLANE) + r"[^']*)'", flat)
    if not subjects or subjects.group(1).split(",") != wanted_subjects:
        fail("%s stream subjects %r differ from audit-core's %r (order matters)" % (
            name, subjects.group(1).split(",") if subjects else None, wanted_subjects))
    for needle in ("--max-msgs=2000000", "--max-age=720h", "--storage=file", "--retention=limits", "--discard=old"):
        if needle not in flat:
            fail("%s stream add lacks %s (audit-core: 30 d, 2M msgs, limits, discard old, file)" % (name, needle))
    for consumer in consumers:
        # The scripts build the durable and filter from the kind; check the pieces.
        if "audit-core-%s-v3-" % PLANE not in text:
            fail("%s does not create the audit-core-%s-v3-* durables" % (name, PLANE))
        expected_filter = consumer["filter"].replace(consumer["kind"], "$1" if "add_audit_consumer" in text else "$kind")
        expected_deliver = consumer["deliver"].replace(consumer["kind"] + "-v2", ("$1" if "add_audit_consumer" in text else "$kind") + "-v2")
        if expected_filter not in text:
            fail("%s filter does not derive %s" % (name, consumer["filter"]))
        if expected_deliver not in text:
            fail("%s delivery subject does not derive %s" % (name, consumer["deliver"]))
    for needle in ("--ack=explicit", "--wait=30s", "--max-deliver=5", "--deliver=all", "--replay=instant"):
        if needle not in flat:
            fail("%s audit durables lack %s (audit-core refuses a durable that differs)" % (name, needle))
    if "VEREVON_AUDIT:verevon.audit.>" in text or "VEREVON_AUDIT verevon.audit.>" in text:
        fail("%s still creates VEREVON_AUDIT on verevon.audit.>, which overlaps %s" % (name, stream_name))
    if "stream edit VEREVON_AUDIT" not in flat or "verevon.audit.retired.>" not in flat:
        fail("%s does not narrow an existing VEREVON_AUDIT" % name)

# ---- 2. No two streams the scripts provision may overlap. ----
streams = {}
for name, text in scripts.items():
    streams[name] = {}
    for match in re.finditer(r'"(VEREVON_[A-Z]+)[: ](verevon\.[a-z.>*]+)"', text):
        streams[name][match.group(1)] = [match.group(2)]
    streams[name][stream_name] = wanted_subjects
    streams[name]["VEREVON_AUDIT (narrowed)"] = ["verevon.audit.retired.>"]
    seen = list(streams[name].items())
    for index, (left, left_subjects) in enumerate(seen):
        for right, right_subjects in seen[index + 1:]:
            for a in left_subjects:
                for b in right_subjects:
                    if subjects_overlap(a, b):
                        fail("%s: streams %s (%s) and %s (%s) overlap" % (name, left, a, right, b))

# The overlap this slice exists to remove: the old stream against the new one.
if not any(subjects_overlap("verevon.audit.>", subject) for subject in wanted_subjects):
    fail("self-check: the old verevon.audit.> should overlap the audit-core stream")

# ---- 3. nats.conf lets the right principals do exactly this. ----
def user_block(user):
    match = re.search(r'user:\s*"' + re.escape(user) + r'"(.*?)\n    \}', nats_conf, re.S)
    if not match:
        fail("nats.conf has no user %s" % user)
        return ""
    return match.group(1)


audit_user = user_block("audit-core-" + PLANE)
for consumer in consumers:
    for needle in (
        "$JS.API.CONSUMER.INFO.%s.%s" % (stream_name, consumer["durable"]),
        "$JS.ACK.%s.%s.>" % (stream_name, consumer["durable"]),
        "%s %s" % (consumer["deliver"], consumer["durable"]),
    ):
        if needle not in audit_user:
            fail("nats.conf user audit-core-%s lacks %s" % (PLANE, needle))
for forbidden in ("$JS.API.STREAM", "$JS.API.CONSUMER.CREATE", "$JS.API.CONSUMER.DURABLE.CREATE"):
    if forbidden in audit_user:
        fail("nats.conf user audit-core-%s must only bind and acknowledge, found %s" % (PLANE, forbidden))
if "verevon.dlq.audit-core.audit" not in audit_user or "verevon.dlq.audit-core.usage" not in audit_user:
    fail("nats.conf user audit-core-%s cannot publish its dead-letter subjects" % PLANE)
provisioner_user = user_block("observability-provisioner-" + PLANE)
if "$JS.API.STREAM.>" not in provisioner_user or "$JS.API.CONSUMER.>" not in provisioner_user:
    fail("nats.conf provisioner principal cannot administer streams and consumers")

# Every audit publisher on this broker lands in the audit-core stream.
publishers = [
    match.group(1)
    for match in re.finditer(r'"(verevon\.audit\.v[12]\.%s\.[^"]+)"' % PLANE, nats_conf)
    if match.group(1) not in ("verevon.audit.v1.%s.>" % PLANE, "verevon.audit.v2.%s.>" % PLANE)
]
if not publishers:
    fail("nats.conf lists no audit publisher: update this test")
for subject in publishers:
    if not any(subjects_overlap(subject, captured) and subject_matches(captured, subject.replace(">", "x")) for captured in wanted_subjects):
        fail("nats.conf lets a principal publish %s but no audit-core stream subject captures it" % subject)

# ---- 4. Runtime streams and shared durables converge to audit-core's values. ----
# Control's ProvisionApplicationRuntime converges VEREVON_APPLICATION, VEREVON_INGESTION and
# VEREVON_MODEL and the push durables below on every run, and refuses an existing durable
# whose max-deliver or ack-wait differs. The scripts create and converge the same resources,
# so their values must match or each run undoes the other (D1-803). The container script
# creates four of the five durables; the host script creates all five.
CONTAINER_SHARED = {
    "conversation-core-ai-action-executor",
    "conversation-core-model-action-proposed",
    "conversation-core-webhook-received",
    "insight-core-metric-subscriber",
}
app_runtime = re.search(r"func ProvisionApplicationRuntime\(.*?\n\}\n", provisioner, re.S)
ensure_runtime = re.search(r"func ensureRuntimeStream\(.*?\n\}\n", provisioner, re.S)
push_consumer = re.search(r"func applicationPushConsumer\(.*?\n\}\n", provisioner, re.S)
if not (app_runtime and ensure_runtime and push_consumer):
    fail("audit-core's provisioner no longer defines the application runtime functions: update this test")
else:
    app_text = app_runtime.group(0)
    ages = re.findall(r"ensureRuntimeStream\(js, stream\.name, stream\.subjects, (\d+)\*24\*time\.Hour, 0\)", app_text)
    msgs = re.findall(r"MaxMsgs\s*[:=]\s*([\d_]+)", ensure_runtime.group(0))
    waits = re.findall(r"AckWait:\s*(\d+)\s*\*\s*time\.Second", push_consumer.group(0))
    delivers = re.findall(r"MaxDeliver:\s*(\d+)", push_consumer.group(0))
    if len(ages) != 1 or len(set(msgs)) != 1 or waits != ["30"] or delivers != ["5"]:
        fail("audit-core's runtime-stream or push-consumer values changed shape (ages %r, messages %r, ack-wait %r, max-deliver %r): update this test" % (ages, msgs, waits, delivers))
    else:
        max_age = "%dh" % (int(ages[0]) * 24)
        max_age_ns = int(ages[0]) * 24 * 3600 * 10 ** 9
        max_msgs = int(msgs[0].replace("_", ""))
        go_streams = {
            const(stream_const): subject
            for stream_const, subject in re.findall(r'\{name: (\w+), subjects: \[\]string\{"([^"]+)"\}\}', app_text)
        }
        go_consumers = {}
        for stream_const, durable_const, delivery_const, filter_value in re.findall(
                r'\{stream: (\w+), config: applicationPushConsumer\(\s*(\w+),\s*(\w+),\s*"([^"]+)"', app_text):
            go_consumers[const(durable_const)] = (const(stream_const), filter_value, const(delivery_const))
        if len(go_streams) != 3 or len(go_consumers) != 5:
            fail("audit-core's application runtime lists changed shape: update this test")
        entry_text = scripts["scripts/jetstream-provision-entrypoint.sh"]
        host_text = scripts["scripts/provision-jetstream.sh"]
        entry_streams = dict(re.findall(r'"(VEREVON_[A-Z]+) (verevon\.[a-z.>*]+)"', entry_text))
        host_streams = dict(re.findall(r'"(VEREVON_[A-Z]+):(verevon\.[a-z.>*]+)"', host_text))
        entry_shared = {}
        for stream_name, durable_name, filter_value, delivery_name in re.findall(
                r'^add_shared_consumer\s+(VEREVON_[A-Z_]+)\s+(\S+)\s+"?([^"\s]+)"?\s+(\S+)\s*$', entry_text, re.M):
            entry_shared[durable_name] = (stream_name, filter_value, delivery_name)
        host_shared = {}
        shared_block = re.search(r"<<'SHARED'\n(.*?)\nSHARED\n", host_text, re.S)
        if not shared_block:
            fail("scripts/provision-jetstream.sh no longer has its SHARED consumer list: update this test")
        for line in (shared_block.group(1).split("\n") if shared_block else []):
            parts = line.split("|")
            if len(parts) != 4:
                fail("scripts/provision-jetstream.sh has a shared consumer line this test cannot read: %r" % line)
                continue
            host_shared[parts[1]] = (parts[0], parts[2], parts[3])
        for label, found_streams, found_shared, required in (
                ("scripts/jetstream-provision-entrypoint.sh", entry_streams, entry_shared, CONTAINER_SHARED),
                ("scripts/provision-jetstream.sh", host_streams, host_shared, set(go_consumers))):
            for stream_name, subject in go_streams.items():
                if found_streams.get(stream_name) != subject:
                    fail("%s does not create %s on %s (audit-core's ProvisionApplicationRuntime)" % (label, stream_name, subject))
            if set(found_shared) != required:
                fail("%s creates shared durables %s; audit-core's list is %s" % (label, sorted(found_shared), sorted(required)))
            for durable_name, value in found_shared.items():
                if durable_name in go_consumers and value != go_consumers[durable_name]:
                    fail("%s creates %s as %r; audit-core's provisioner wants %r" % (label, durable_name, value, go_consumers[durable_name]))
        for label, text, function in (
                ("scripts/jetstream-provision-entrypoint.sh", entry_text, "add_shared_consumer"),
                ("scripts/provision-jetstream.sh", host_text, "ensure_shared_consumer")):
            body_match = re.search(r"^" + function + r"\(\) \{\n(.*?)\n\}$", text, re.M | re.S)
            if not body_match:
                fail("%s no longer defines %s: update this test" % (label, function))
                continue
            body = re.sub(r"\\\n\s*", " ", body_match.group(1))
            if "--wait=30s --max-deliver=5" not in body or not re.search(r"consumer edit[^\n]*--max-deliver=5 --wait=30s --force", body):
                fail("%s must create and converge shared durables with --wait=30s --max-deliver=5" % label)
            if "max_deliver 5" not in body or "ack_wait %d" % (30 * 10 ** 9) not in body:
                fail("%s must compare shared durables against max-deliver 5 and ack-wait 30 s" % label)
        for label, text in (
                ("scripts/jetstream-provision-entrypoint.sh", entry_text),
                ("scripts/provision-jetstream.sh", host_text)):
            flat = re.sub(r"\\\n\s*", " ", text)
            if not re.search(r"stream add[^\n]*--max-msgs=%d[^\n]*--max-age=%s" % (max_msgs, max_age), flat):
                fail("%s creates the runtime streams without --max-msgs=%d --max-age=%s" % (label, max_msgs, max_age))
            if not re.search(r"stream edit[^\n]*--max-age=%s[^\n]*--max-msgs=%d[^\n]*--force" % (max_age, max_msgs), flat):
                fail("%s does not converge an existing runtime stream with stream edit to --max-age=%s --max-msgs=%d" % (label, max_age, max_msgs))
            if "max_age %d" % max_age_ns not in flat or "max_msgs %d" % max_msgs not in flat:
                fail("%s does not compare the runtime streams against max_age %d and max_msgs %d" % (label, max_age_ns, max_msgs))


if failures:
    for message in failures:
        sys.stderr.write("Application Plane audit stream contract: %s\n" % message)
    sys.exit(1)
print("Application Plane audit stream contract: ok (%d subjects, %d durables, %d publishers)" % (
    len(wanted_subjects), len(consumers), len(publishers)))
PY
