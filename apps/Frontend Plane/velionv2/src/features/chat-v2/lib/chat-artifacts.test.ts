import { describe, expect, it } from "vitest";
import { imageArtifactSrc, selectLatestImageArtifact } from "./chat-artifacts";
import type { ChatMessage } from "./chat-workspace";

function msg(artifacts: Array<{ id: string; kind: string }>): ChatMessage {
  return {
    id: artifacts[0]?.id ?? "m",
    role: "assistant",
    content: "",
    createdAt: "2026-01-01T00:00:00Z",
    tools: [],
    attachments: [],
    artifacts: artifacts.map((a) => ({
      id: a.id,
      kind: a.kind,
      title: a.id,
      content: a.id,
      version: 1,
    })),
  };
}

describe("imageArtifactSrc", () => {
  it("passes through http(s) URLs", () => {
    expect(imageArtifactSrc("https://cdn/x.png")).toBe("https://cdn/x.png");
    expect(imageArtifactSrc("http://cdn/x.png")).toBe("http://cdn/x.png");
  });

  it("passes through data URLs", () => {
    const dataUrl = "data:image/png;base64,AAAA";
    expect(imageArtifactSrc(dataUrl)).toBe(dataUrl);
  });

  it("wraps bare base64 as a PNG data URL", () => {
    expect(imageArtifactSrc("AAAABBBB")).toBe("data:image/png;base64,AAAABBBB");
  });

  it("trims surrounding whitespace before deciding", () => {
    expect(imageArtifactSrc("  https://cdn/x.png  ")).toBe("https://cdn/x.png");
  });
});

describe("selectLatestImageArtifact", () => {
  it("returns the most recent image across messages", () => {
    const messages = [
      msg([{ id: "shot-1", kind: "image" }]),
      msg([{ id: "doc-1", kind: "markdown" }]),
      msg([{ id: "shot-2", kind: "image" }, { id: "code-1", kind: "code" }]),
    ];
    expect(selectLatestImageArtifact(messages)?.id).toBe("shot-2");
  });

  it("returns null when no image artifact exists", () => {
    expect(selectLatestImageArtifact([msg([{ id: "doc-1", kind: "markdown" }])])).toBeNull();
    expect(selectLatestImageArtifact([])).toBeNull();
  });
});
