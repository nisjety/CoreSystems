import { describe, expect, it } from "vitest";
import {
  buildAudioAppend,
  buildAudioCommit,
  buildSessionUpdate,
  parseServerEvent,
} from "./voice-protocol";
import { int16ToBase64 } from "./voice-audio";

describe("voice-protocol builders", () => {
  it("builds a session.update with pcm16 audio formats", () => {
    const ev = buildSessionUpdate({ voice: "verse", instructions: "be brief" }) as {
      type: string;
      session: Record<string, unknown>;
    };
    expect(ev.type).toBe("session.update");
    expect(ev.session.voice).toBe("verse");
    expect(ev.session.input_audio_format).toBe("pcm16");
    expect(ev.session.output_audio_format).toBe("pcm16");
  });

  it("builds input_audio_buffer.append with base64 PCM16", () => {
    const frame = new Int16Array([1, -1, 256]);
    const ev = buildAudioAppend(frame) as { type: string; audio: string };
    expect(ev.type).toBe("input_audio_buffer.append");
    expect(ev.audio).toBe(int16ToBase64(frame));
  });

  it("builds input_audio_buffer.commit", () => {
    expect(buildAudioCommit()).toEqual({ type: "input_audio_buffer.commit" });
  });
});

describe("voice-protocol parser", () => {
  it("parses an audio delta back to PCM16 samples", () => {
    const pcm = new Int16Array([5, -7, 9]);
    const raw = JSON.stringify({ type: "response.audio.delta", delta: int16ToBase64(pcm) });
    const ev = parseServerEvent(raw);
    expect(ev.kind).toBe("audio");
    if (ev.kind === "audio") {
      expect(Array.from(ev.pcm)).toEqual([5, -7, 9]);
    }
  });

  it("parses transcript and text deltas", () => {
    expect(parseServerEvent(JSON.stringify({ type: "response.audio_transcript.delta", delta: "he" }))).toEqual({
      kind: "transcript",
      text: "he",
    });
    expect(parseServerEvent(JSON.stringify({ type: "response.text.delta", delta: "lo" }))).toEqual({
      kind: "text",
      text: "lo",
    });
  });

  it("parses done and error events", () => {
    expect(parseServerEvent(JSON.stringify({ type: "response.done" }))).toEqual({ kind: "done" });
    expect(parseServerEvent(JSON.stringify({ type: "error", error: { message: "boom" } }))).toEqual({
      kind: "error",
      message: "boom",
    });
  });

  it("degrades on malformed JSON and unknown types", () => {
    expect(parseServerEvent("not json").kind).toBe("error");
    expect(parseServerEvent(JSON.stringify({ type: "session.created" }))).toEqual({
      kind: "other",
      type: "session.created",
    });
  });
});
