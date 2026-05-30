// Wave 10 smoke probe.
//
// Uses native Node HTTP/2 + manual protobuf encoding because grpcurl
// has a local interop quirk on this macOS dev box (silently hangs on
// tonic responses). Same trick we used for the Wave 9 smoke.
//
// Encodes only a small subset of fields per request — enough to exercise
// the RPC + see a successful response shape. No proto parser needed; we
// hand-write tag bytes for the few fields each call uses.
//
// Run:
//   node apps/Model\ Plane/scripts/smoke-wave10.js
//
// Expected: every row prints "OK" with grpc-status=0 and a non-empty
// response body. Failures print "FAIL" with the gRPC status code.

const http2 = require("http2");

const GATEWAY = process.env.GATEWAY || "http://localhost:9090";
const TIMEOUT_MS = 8000;

// ---- Protobuf primitives (wire format only — no schema) -----------------
function tagWire(field, wire) { return (field << 3) | wire; }
function encVarint(v) {
  const out = [];
  while (v > 127) { out.push((v & 0x7f) | 0x80); v >>>= 7; }
  out.push(v);
  return Buffer.from(out);
}
function encString(field, s) {
  const bytes = Buffer.from(s, "utf-8");
  return Buffer.concat([Buffer.from([tagWire(field, 2)]), encVarint(bytes.length), bytes]);
}
function encInt32(field, n) {
  return Buffer.concat([Buffer.from([tagWire(field, 0)]), encVarint(n)]);
}
function encBool(field, b) {
  return Buffer.concat([Buffer.from([tagWire(field, 0)]), encVarint(b ? 1 : 0)]);
}
function encMessage(field, payload) {
  return Buffer.concat([
    Buffer.from([tagWire(field, 2)]),
    encVarint(payload.length),
    payload,
  ]);
}

// Minimal wire-format decoder — pulls out top-level string + int fields
// we care about. Returns a {fieldNumber: value} map; strings prefer the
// last value if a tag repeats.
function decodeMessage(buf) {
  const out = {};
  let off = 0;
  while (off < buf.length) {
    const tag = buf[off++];
    const fn = tag >>> 3;
    const wire = tag & 7;
    if (wire === 0) {
      // varint
      let v = 0n, shift = 0n;
      while (true) {
        const b = buf[off++];
        v |= BigInt(b & 0x7f) << shift;
        if ((b & 0x80) === 0) break;
        shift += 7n;
      }
      out[fn] = Number(v);
    } else if (wire === 2) {
      let len = 0n, shift = 0n;
      while (true) {
        const b = buf[off++];
        len |= BigInt(b & 0x7f) << shift;
        if ((b & 0x80) === 0) break;
        shift += 7n;
      }
      const lenN = Number(len);
      out[fn] = buf.slice(off, off + lenN);
      off += lenN;
    } else if (wire === 1) {
      out[fn] = buf.slice(off, off + 8); // fixed64 / double
      off += 8;
    } else if (wire === 5) {
      out[fn] = buf.slice(off, off + 4); // fixed32 / float
      off += 4;
    } else {
      // Unsupported wire type — bail.
      break;
    }
  }
  return out;
}

// ---- gRPC over HTTP/2 ---------------------------------------------------
function call(path, payload) {
  return new Promise((resolve) => {
    const client = http2.connect(GATEWAY);
    const t0 = Date.now();
    const req = client.request({
      ":method": "POST",
      ":path": path,
      "content-type": "application/grpc",
      "te": "trailers",
    });
    const chunks = [];
    let trailers = null;
    req.on("data", (c) => chunks.push(c));
    req.on("trailers", (t) => { trailers = t; });
    req.on("end", () => {
      client.close();
      resolve({ trailers, body: Buffer.concat(chunks), elapsed: Date.now() - t0 });
    });
    req.on("error", (e) => { client.close(); resolve({ error: e.message }); });
    setTimeout(() => { client.close(); resolve({ error: `timeout ${TIMEOUT_MS}ms` }); }, TIMEOUT_MS);
    // gRPC frame = flag(1) + length(4 BE) + payload
    const hdr = Buffer.alloc(5);
    hdr.writeUInt32BE(payload.length, 1);
    req.end(Buffer.concat([hdr, payload]));
  });
}

function unwrap(body) {
  if (body.length < 5) return null;
  const len = body.readUInt32BE(1);
  return body.slice(5, 5 + len);
}

// ---- Smoke cases --------------------------------------------------------

async function smoke(name, path, payload, expectFields = []) {
  const r = await call(path, payload);
  if (r.error) {
    console.log(`FAIL ${name}: transport ${r.error}`);
    return false;
  }
  const status = r.trailers?.["grpc-status"];
  const message = r.trailers?.["grpc-message"];
  if (status !== "0") {
    console.log(`FAIL ${name}: grpc-status=${status} message=${message || "(none)"}`);
    return false;
  }
  const decoded = decodeMessage(unwrap(r.body) || Buffer.alloc(0));
  const extras = expectFields.map((f) => {
    const v = decoded[f];
    if (Buffer.isBuffer(v)) return `f${f}=${v.toString("utf-8").slice(0, 40)}`;
    return `f${f}=${v}`;
  });
  console.log(`OK   ${name} (${r.elapsed}ms) ${extras.join(" ")}`);
  return true;
}

(async () => {
  console.log(`Smoke probe → ${GATEWAY}`);

  // 1. Health (existing, sanity)
  await smoke("Health", "/model_plane.v1.ModelGateway/Health", Buffer.alloc(0), [1]);

  // 2. Wave 10a — Sleep (5 ms)
  await smoke(
    "Sleep",
    "/model_plane.v1.ModelGateway/Sleep",
    Buffer.concat([
      encString(1, "smoke-sleep"),  // request_id
      encString(2, "smoke-org"),    // org_id
      encInt32(3, 5),               // duration_ms
    ]),
    [1, 2],
  );

  // 3. Wave 10a — SyntheticOutput (echoes payload)
  await smoke(
    "SyntheticOutput",
    "/model_plane.v1.ModelGateway/SyntheticOutput",
    Buffer.concat([
      encString(1, "smoke-syn"),
      encString(2, "smoke-org"),
      encString(3, "hello-wave-10"),
    ]),
    [1, 2],
  );

  // 4. Wave 10b — EnterPlanMode (200s TTL)
  await smoke(
    "EnterPlanMode",
    "/model_plane.v1.ModelGateway/EnterPlanMode",
    Buffer.concat([
      encString(1, "smoke-plan"),
      encString(2, "smoke-org"),
      encString(3, "run-1"),
      encString(4, "sess-1"),
      encString(5, "drafting"),
      encInt32(6, 200),
    ]),
    [1, 2],
  );

  // 5. Wave 10b — IsPlanMode should return active=true
  await smoke(
    "IsPlanMode",
    "/model_plane.v1.ModelGateway/IsPlanMode",
    Buffer.concat([
      encString(1, "smoke-plan-q"),
      encString(2, "run-1"),
    ]),
    [1, 2, 3, 4],
  );

  // 6. Wave 10b — TeamCreate
  await smoke(
    "TeamCreate",
    "/model_plane.v1.ModelGateway/TeamCreate",
    Buffer.concat([
      encString(1, "smoke-team"),
      encString(2, "smoke-org"),
      encString(3, "Investigate the bug"),
    ]),
    [1],
  );

  // 7. Wave 10d — ListSkills (empty, but should succeed)
  await smoke(
    "ListSkills",
    "/model_plane.v1.ModelGateway/ListSkills",
    Buffer.concat([
      encString(1, "smoke-list-skills"),
      encString(2, "smoke-org"),
    ]),
    [1],
  );

  // 8. Wave 10g — RegisterMcpServer
  const mcpServer = Buffer.concat([
    encString(2, "my-bridge"),                   // name
    encString(3, "http://example.invalid"),      // url
    encString(4, "http"),                        // transport
    encBool(7, true),                            // enabled
  ]);
  await smoke(
    "RegisterMcpServer",
    "/model_plane.v1.ModelGateway/RegisterMcpServer",
    Buffer.concat([
      encString(1, "smoke-mcp"),
      encString(2, "smoke-org"),
      encMessage(3, mcpServer),
    ]),
    [1],
  );

  // 9. Wave 10h — RegisterPlugin
  const plugin = Buffer.concat([
    encString(2, "my-plugin"),  // name
    encString(3, "1.0.0"),       // version
    encString(4, "tool"),        // kind
    encBool(6, true),            // enabled
  ]);
  await smoke(
    "RegisterPlugin",
    "/model_plane.v1.ModelGateway/RegisterPlugin",
    Buffer.concat([
      encString(1, "smoke-plugin"),
      encString(2, "smoke-org"),
      encMessage(3, plugin),
    ]),
    [1],
  );

  // 10. Wave 10i — CheckPermission (default-open)
  await smoke(
    "CheckPermission",
    "/model_plane.v1.ModelGateway/CheckPermission",
    Buffer.concat([
      encString(1, "smoke-perm"),
      encString(2, "smoke-org"),
      encString(3, "bash"),
    ]),
    [1, 2, 3],
  );

  // 11. Wave 10i — GetPolicy (default)
  await smoke(
    "GetPolicy",
    "/model_plane.v1.ModelGateway/GetPolicy",
    Buffer.concat([
      encString(1, "smoke-policy"),
      encString(2, "smoke-org"),
    ]),
    [1],
  );

  // 12. Wave 10j — AppendThreadMessage
  await smoke(
    "AppendThreadMessage",
    "/model_plane.v1.ModelGateway/AppendThreadMessage",
    Buffer.concat([
      encString(1, "smoke-msg"),
      encString(2, "smoke-org"),
      encString(3, "thread-1"),
      encString(4, "user"),
      encString(5, "hello, wave 10"),
    ]),
    [1],
  );

  // 13. Wave 10j — GetAnalytics (zero counters)
  await smoke(
    "GetAnalytics",
    "/model_plane.v1.ModelGateway/GetAnalytics",
    Buffer.concat([
      encString(1, "smoke-an"),
      encString(2, "smoke-org"),
    ]),
    [1, 2, 3],
  );

  // 14. Wave 10j — CreateTask
  await smoke(
    "CreateTask",
    "/model_plane.v1.ModelGateway/CreateTask",
    Buffer.concat([
      encString(1, "smoke-task"),
      encString(2, "smoke-org"),
      encString(3, "smoke task description"),
    ]),
    [1],
  );

  // 15. Wave 10f — RecordTrajectory + ListTrajectories
  const traj = Buffer.concat([
    encString(2, "smoke-org"),
    encString(3, "run-X"),
    encString(5, "smoke goal"),
    encString(8, "success"),
  ]);
  await smoke(
    "RecordTrajectory",
    "/model_plane.v1.ModelGateway/RecordTrajectory",
    Buffer.concat([
      encString(1, "smoke-rec"),
      encMessage(2, traj),
    ]),
    [1, 2, 3],
  );
  await smoke(
    "ListTrajectories",
    "/model_plane.v1.ModelGateway/ListTrajectories",
    Buffer.concat([
      encString(1, "smoke-list-traj"),
      encString(2, "smoke-org"),
    ]),
    [1, 3],
  );

  console.log("done.");
})();
