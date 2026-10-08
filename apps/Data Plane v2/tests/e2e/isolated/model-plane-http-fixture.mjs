import { createHash } from "node:crypto";
import { createServer } from "node:http";
import { createServer as createHttp2Server } from "node:http2";

const dimension = positiveInteger(process.env.EMBEDDING_DIMENSION ?? "32");
const port = positiveInteger(process.env.PORT ?? "8088");
// inference-core's gRPC port. graph-index reaches a model through inference-core and nowhere
// else (its direct-Azure backend is deleted), so the deterministic graph extraction this stack
// needs is answered here, at the Model Plane boundary, over the real wire contract.
const grpcPort = positiveInteger(process.env.GRPC_PORT ?? "9092");
const markerPattern = /GRAPH_NODE_[A-Za-z0-9_-]{1,96}/;
const INFER_PATH = "/model_plane.v1.InferenceCore/Infer";
const GRAPH_MODEL = "isolated-deterministic-graph";

const server = createServer(async (request, response) => {
  if (request.method === "GET" && request.url === "/health") {
    return sendJson(response, 200, { status: "ok" });
  }
  if (request.method !== "POST" || !request.url?.startsWith("/openai/deployments/")) {
    return sendJson(response, 404, { error: { message: "not found" } });
  }

  const body = await readJson(request);
  if (request.url.includes("/embeddings?")) {
    const values = Array.isArray(body.input) ? body.input : [body.input ?? ""];
    return sendJson(response, 200, {
      data: values.map((value, index) => ({
        embedding: deterministicVector(String(value), dimension),
        index,
        object: "embedding",
      })),
      model: "isolated-deterministic-embedding",
      object: "list",
      usage: { prompt_tokens: 1, total_tokens: 1 },
    });
  }
  return sendJson(response, 404, { error: { message: "unsupported fixture operation" } });
});

server.listen(port, "0.0.0.0");

// ── inference-core, over gRPC ────────────────────────────────────────────────
//
// `model_plane.v1.InferenceCore/Infer`, as graph-index's tonic client calls it: HTTP/2 without
// TLS, one length-prefixed protobuf message each way, `grpc-status` in the trailers. Only what
// graph-index reads is encoded. A call without a bearer is refused UNAUTHENTICATED, as
// inference-core refuses it: the stack must have minted one through the real Auth Core.

const grpc = createHttp2Server();
grpc.on("stream", (stream, headers) => {
  const chunks = [];
  stream.on("data", (chunk) => chunks.push(chunk));
  stream.on("error", () => {});
  stream.on("end", () => {
    if (headers[":path"] !== INFER_PATH) {
      return grpcFailure(stream, 12, "unimplemented fixture operation");
    }
    if (!/^Bearer [A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]*$/.test(String(headers.authorization ?? ""))) {
      return grpcFailure(stream, 16, "a bearer issued by Auth Core is required");
    }
    const frame = Buffer.concat(chunks);
    if (frame.length < 5 || frame[0] !== 0 || frame.readUInt32BE(1) !== frame.length - 5) {
      return grpcFailure(stream, 3, "malformed gRPC message");
    }
    const message = frame.subarray(5);
    // The marker the seeded document carries travels inside the prompt, as UTF-8.
    const marker = message.toString("utf8").match(markerPattern)?.[0] ?? "GRAPH_NODE_ISOLATED";
    return grpcReply(
      stream,
      encodeInferResponse({
        requestId: readRequestId(message),
        content: graphExtraction(marker),
        model: GRAPH_MODEL,
        provider: "isolated-fixture",
        inputTokens: 1,
        outputTokens: 1,
      }),
    );
  });
});
grpc.listen(grpcPort, "0.0.0.0");

/** The extraction graph-index expects, for the marker a seeded document carries. */
function graphExtraction(marker) {
  return JSON.stringify({
    entities: [
      { entity_type: "product", entity_text: marker, confidence: 0.99 },
      { entity_type: "policy", entity_text: `${marker}_POLICY`, confidence: 0.96 },
    ],
    relationships: [
      {
        source_entity: marker,
        target_entity: `${marker}_POLICY`,
        relation_type: "documents",
        confidence: 0.94,
      },
    ],
    claims: [
      {
        claim_text: `${marker} is grounded by the isolated browser fixture`,
        related_entities: [marker, `${marker}_POLICY`],
        confidence: 0.97,
      },
    ],
  });
}

function grpcReply(stream, message) {
  const frame = Buffer.alloc(5 + message.length);
  frame.writeUInt32BE(message.length, 1);
  message.copy(frame, 5);
  stream.respond(
    { ":status": 200, "content-type": "application/grpc", "grpc-encoding": "identity" },
    { waitForTrailers: true },
  );
  stream.on("wantTrailers", () => stream.sendTrailers({ "grpc-status": "0" }));
  stream.end(frame);
}

function grpcFailure(stream, code, message) {
  stream.respond(
    {
      ":status": 200,
      "content-type": "application/grpc",
      "grpc-status": String(code),
      "grpc-message": encodeURIComponent(message),
    },
    { endStream: true },
  );
}

// ── just enough protobuf ─────────────────────────────────────────────────────

/** `InferRequest.request_id` (field 1), the first field graph-index writes; empty when absent. */
function readRequestId(message) {
  if (message.length < 2 || message[0] !== 0x0a) return "";
  const [length, offset] = readVarint(message, 1);
  return message.subarray(offset, offset + length).toString("utf8");
}

/**
 * `InferResponse` (model_plane/v1/inference.proto): request_id = 1, content = 2,
 * model_used = 3, input_tokens = 5, output_tokens = 6, provider_used = 8, and
 * provider_timing = 14 with usage_reported (field 10) set, which is what makes the call count as
 * a metered call with exact numbers.
 */
function encodeInferResponse({ requestId, content, model, provider, inputTokens, outputTokens }) {
  const text = (field, value) => {
    const bytes = Buffer.from(value, "utf8");
    return Buffer.concat([Buffer.from([(field << 3) | 2]), varint(bytes.length), bytes]);
  };
  const number = (field, value) => Buffer.concat([Buffer.from([(field << 3) | 0]), varint(value)]);
  const timing = Buffer.concat([number(10, 1)]);
  return Buffer.concat([
    text(1, requestId),
    text(2, content),
    text(3, model),
    number(5, inputTokens),
    number(6, outputTokens),
    text(8, provider),
    Buffer.concat([Buffer.from([(14 << 3) | 2]), varint(timing.length), timing]),
  ]);
}

function varint(value) {
  const bytes = [];
  let rest = value;
  while (rest > 0x7f) {
    bytes.push((rest & 0x7f) | 0x80);
    rest = Math.floor(rest / 128);
  }
  bytes.push(rest);
  return Buffer.from(bytes);
}

function readVarint(buffer, start) {
  let value = 0;
  let shift = 1;
  let offset = start;
  for (;;) {
    const byte = buffer[offset++];
    value += (byte & 0x7f) * shift;
    if (byte < 0x80) return [value, offset];
    shift *= 128;
  }
}

function deterministicVector(text, size) {
  const digest = createHash("sha256").update(text).digest();
  const vector = Array.from({ length: size }, (_, index) => {
    const byte = digest[index % digest.length];
    return (byte - 127.5) / 127.5;
  });
  const norm = Math.sqrt(vector.reduce((sum, value) => sum + value * value, 0)) || 1;
  return vector.map((value) => value / norm);
}

async function readJson(request) {
  const chunks = [];
  let size = 0;
  for await (const chunk of request) {
    size += chunk.length;
    if (size > 1_048_576) throw new Error("fixture request too large");
    chunks.push(chunk);
  }
  return JSON.parse(Buffer.concat(chunks).toString("utf8") || "{}");
}

function sendJson(response, status, body) {
  const encoded = JSON.stringify(body);
  response.writeHead(status, {
    "content-type": "application/json",
    "content-length": Buffer.byteLength(encoded),
    "cache-control": "no-store",
  });
  response.end(encoded);
}

function positiveInteger(value) {
  const parsed = Number.parseInt(value, 10);
  if (!Number.isSafeInteger(parsed) || parsed <= 0) throw new Error("positive integer required");
  return parsed;
}
