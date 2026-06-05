import { describe, expect, it } from "vitest";
import { imageArtifactSrc } from "./chat-artifacts";

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
