import { describe, expect, it } from "vitest";
import { parseEventJson, parseFrame } from "./sse";

describe("sse parser", () => {
  it("parses a named event with a JSON data line", () => {
    const frame = "event: snippet\ndata: {\"id\":\"1\",\"title\":\"Pricing\"}";
    const parsed = parseFrame(frame);
    expect(parsed).toEqual({ event: "snippet", data: '{"id":"1","title":"Pricing"}' });
  });

  it("defaults the event name to 'message' when omitted", () => {
    expect(parseFrame("data: hello")).toEqual({ event: "message", data: "hello" });
  });

  it("returns null for a dataless frame", () => {
    expect(parseFrame("event: ping")).toBeNull();
  });

  it("concatenates multiple data lines", () => {
    expect(parseFrame("data: a\ndata: b")).toEqual({ event: "message", data: "ab" });
  });

  it("decodes event JSON and returns null on malformed payloads", () => {
    expect(parseEventJson({ event: "x", data: '{"n":2}' })).toEqual({ n: 2 });
    expect(parseEventJson({ event: "x", data: "not json" })).toBeNull();
  });
});
