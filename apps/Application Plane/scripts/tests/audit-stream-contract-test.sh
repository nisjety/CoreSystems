#!/usr/bin/env bash
# Contract test for the Application Plane's audit-core stream (plan row WS01-S4).
#
# Mirrors apps/Model Plane/go/services/nats-provisioner/manifest_contract_test.go
# for the application broker: audit-core's provisioner (Control Plane) and this
# plane's two provisioning scripts both converge VEREVON_CONTROL_OBSERVABILITY, so
# any difference makes each undo the other, and any overlapping subject in another
# stream (the old VEREVON_AUDIT, verevon.audit.>) makes the broker refuse the
# stream outright. Static: reads files only, needs no broker.
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

if failures:
    for message in failures:
        sys.stderr.write("Application Plane audit stream contract: %s\n" % message)
    sys.exit(1)
print("Application Plane audit stream contract: ok (%d subjects, %d durables, %d publishers)" % (
    len(wanted_subjects), len(consumers), len(publishers)))
PY
