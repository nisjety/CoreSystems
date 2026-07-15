import { createHash } from "node:crypto";
import { createServer } from "node:http";

const dimension = positiveInteger(process.env.EMBEDDING_DIMENSION ?? "32");
const port = positiveInteger(process.env.PORT ?? "8088");
const markerPattern = /GRAPH_NODE_[A-Za-z0-9_-]{1,96}/;

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
  if (request.url.includes("/chat/completions?")) {
    const prompt = Array.isArray(body.messages)
      ? body.messages.map((message) => String(message?.content ?? "")).join("\n")
      : "";
    const marker = prompt.match(markerPattern)?.[0] ?? "GRAPH_NODE_ISOLATED";
    const content = JSON.stringify({
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
    return sendJson(response, 200, {
      choices: [{ index: 0, message: { role: "assistant", content } }],
      model: "isolated-deterministic-graph",
      usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 },
    });
  }
  return sendJson(response, 404, { error: { message: "unsupported fixture operation" } });
});

server.listen(port, "0.0.0.0");

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
