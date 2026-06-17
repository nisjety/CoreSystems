import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { loadKnowledgeDiagnostics } from "@/lib/knowledge/knowledge-diagnostics";

function makeJsonResponse(body: unknown, status = 200): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: () => Promise.resolve(body),
  } as Response;
}

function makeEmptyResponse(status = 200): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: () => Promise.resolve(null),
  } as Response;
}

function findItem<T extends { id: string }>(items: T[], id: string): T {
  const item = items.find((entry) => entry.id === id);
  expect(item).toBeDefined();
  return item as T;
}

describe("loadKnowledgeDiagnostics", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn((input: RequestInfo | URL) => {
      const url = String(input);

      if (url.includes("retrieval-engine") && url.endsWith("/readyz")) {
        return Promise.resolve(makeJsonResponse({
          checks: {
            postgres: true,
            qdrant: true,
            redis: true,
          },
          service: "retrieval-engine-rs",
          sparse_backend: "quickwit-with-postgres-fallback",
          status: "ready",
        }));
      }

      if (url.includes("embedding-engine") && url.endsWith("/readyz")) {
        return Promise.resolve(makeJsonResponse({
          service: "embedding-engine-rs",
          status: "ready",
        }));
      }

      if (url.includes("graph-index") && url.endsWith("/readyz")) {
        return Promise.resolve(makeJsonResponse({
          service: "graph-index-rs",
          status: "ready",
        }));
      }

      if (url.includes("wiki-store") && url.endsWith("/readyz")) {
        return Promise.resolve(makeJsonResponse({
          service: "wiki-store-go",
          status: "ready",
        }));
      }

      if (url.includes("quickwit-adapter") && url.endsWith("/readyz")) {
        return Promise.resolve(makeJsonResponse({
          index: "dataplane-corpus",
          service: "quickwit-adapter-rs",
          status: "ready",
        }));
      }

      if (url.includes("documents-api-go") && url.endsWith("/readyz")) {
        return Promise.resolve(makeJsonResponse({
          service: "documents-api-go",
          status: "ready",
        }));
      }

      if (url.includes("qdrant") && url.endsWith("/collections")) {
        return Promise.resolve(makeJsonResponse({
          result: {
            collections: [
              { name: "dataplane_knowledge" },
              { name: "wiki_block_embeddings" },
              { name: "entity_summary_embeddings" },
            ],
          },
          status: "ok",
        }));
      }

      if (url.includes("quickwit") && url.includes("/api/v1/indexes")) {
        return Promise.resolve(makeJsonResponse([
          {
            index_config: {
              index_id: "dataplane-corpus",
              index_uri: "s3://quickwit/indexes/dataplane-corpus",
            },
          },
        ]));
      }

      if (url.includes("minio") && url.includes("/minio/health/live")) {
        return Promise.resolve(makeEmptyResponse(200));
      }

      return Promise.resolve(makeJsonResponse({ detail: `Unexpected fetch ${url}` }, 404));
    }));
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("derives live dataplane storage and capability status", async () => {
    const diagnostics = await loadKnowledgeDiagnostics({
      documentCount: 24,
      graphAvailable: true,
      graphEdgeCount: 9,
      graphNodeCount: 6,
      indexedCount: 22,
    });

    expect(diagnostics.available).toBe(true);
    expect(diagnostics.sparseBackend).toBe("quickwit-with-postgres-fallback");
    expect(diagnostics.vectorCollections).toEqual(expect.arrayContaining([
      "dataplane_knowledge",
      "wiki_block_embeddings",
      "entity_summary_embeddings",
    ]));
    expect(diagnostics.quickwitIndexes).toEqual(["dataplane-corpus"]);

    expect(findItem(diagnostics.storage, "postgres").status).toBe("Ready");
    expect(findItem(diagnostics.storage, "redis-rag").status).toBe("Ready");
    expect(findItem(diagnostics.storage, "qdrant").status).toBe("Ready");
    expect(findItem(diagnostics.storage, "minio").status).toBe("Ready");

    expect(findItem(diagnostics.capabilities, "embeddings").status).toBe("Live");
    expect(findItem(diagnostics.capabilities, "graphrag").status).toBe("Live");
    expect(findItem(diagnostics.capabilities, "llm-wiki").status).toBe("Live");
    expect(findItem(diagnostics.capabilities, "context-pack").status).toBe("Live");
    expect(findItem(diagnostics.capabilities, "context-mode").status).toBe("Not wired");
    expect(findItem(diagnostics.capabilities, "mempalace").status).toBe("Not wired");
  });
});
