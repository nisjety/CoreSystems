// Smoke TextToSpeech RPC against the model-gateway.
//
// Hand-encodes the request body (proto wire format) and decodes
// the response's audio bytes — avoids the grpcurl-on-macOS hang
// noted in smoke-wave10.js.
//
// Usage:
//   node apps/Model\ Plane/scripts/smoke-tts.js [text]
//
// Reports: grpc-status, audio_bytes, format, magic header,
// and error_message if Azure returned an upstream error.

const http2 = require("http2");

const GATEWAY = process.env.GATEWAY || "http://localhost:9090";
const TIMEOUT_MS = 30000;
const TEXT = process.argv[2] || "hello from azure";

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

function decodeMessage(buf) {
  const out = {};
  let off = 0;
  while (off < buf.length) {
    const tag = buf[off++];
    const fn = tag >>> 3;
    const wire = tag & 7;
    if (wire === 0) {
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
    } else { break; }
  }
  return out;
}

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

(async () => {
  const payload = Buffer.concat([
    encString(1, "smoke-tts"),
    encString(2, "smoke-org"),
    encString(3, TEXT),
    encString(4, "alloy"),
    encString(5, "mp3"),
  ]);
  console.log(`Smoke TTS → ${GATEWAY}  text=${JSON.stringify(TEXT)}`);
  const r = await call("/model_plane.v1.ModelGateway/TextToSpeech", payload);
  if (r.error) {
    console.log(`FAIL transport: ${r.error}`);
    process.exit(1);
  }
  const status = r.trailers?.["grpc-status"];
  const message = r.trailers?.["grpc-message"];
  console.log(`grpc-status=${status} grpc-message=${message || "(none)"} elapsed=${r.elapsed}ms`);
  const inner = unwrap(r.body) || Buffer.alloc(0);
  const decoded = decodeMessage(inner);
  const audio = decoded[2];
  const format = decoded[3];
  const err = decoded[4];
  const audioBytes = Buffer.isBuffer(audio) ? audio.length : 0;
  const magic = Buffer.isBuffer(audio) && audioBytes >= 3
    ? audio.slice(0, 3).toString("hex").toUpperCase()
    : "(none)";
  console.log(`audio_bytes=${audioBytes}`);
  console.log(`format=${Buffer.isBuffer(format) ? format.toString("utf-8") : ""}`);
  console.log(`magic_hex=${magic}  (494433=ID3/mp3, 52494646=RIFF/wav, 4F676753=OggS)`);
  if (Buffer.isBuffer(err) && err.length) {
    console.log(`error_message=${err.toString("utf-8")}`);
  }
  const ok = status === "0" && audioBytes > 0 && !(Buffer.isBuffer(err) && err.length);
  console.log(ok ? "RESULT: OK" : "RESULT: FAIL");
  process.exit(ok ? 0 : 1);
})();
