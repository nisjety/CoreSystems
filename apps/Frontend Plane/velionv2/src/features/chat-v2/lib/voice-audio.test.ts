import { describe, expect, it } from "vitest";
import {
  base64ToInt16,
  floatTo16BitPCM,
  int16ToBase64,
  pcm16ToFloat32,
} from "./voice-audio";

describe("voice-audio DSP helpers", () => {
  it("clamps out-of-range float samples to the PCM16 limits", () => {
    const pcm = floatTo16BitPCM(new Float32Array([2, -2, 0]));
    expect(pcm[0]).toBe(0x7fff); // +1.0 max
    expect(pcm[1]).toBe(-0x8000); // -1.0 min
    expect(pcm[2]).toBe(0);
  });

  it("round-trips float → PCM16 → float within quantization tolerance", () => {
    const input = new Float32Array([0, 0.5, -0.5, 0.25, -0.99]);
    const back = pcm16ToFloat32(floatTo16BitPCM(input));
    for (let i = 0; i < input.length; i += 1) {
      expect(Math.abs(back[i] - input[i])).toBeLessThan(1e-3);
    }
  });

  it("round-trips PCM16 → base64 → PCM16 exactly (little-endian, signed)", () => {
    const samples = new Int16Array([0, 1, -1, 32767, -32768, 1234, -4321]);
    const decoded = base64ToInt16(int16ToBase64(samples));
    expect(Array.from(decoded)).toEqual(Array.from(samples));
  });

  it("encodes a known sample to little-endian base64", () => {
    // 0x0102 → bytes [0x02, 0x01] → base64 "AgE="
    expect(int16ToBase64(new Int16Array([0x0102]))).toBe("AgE=");
  });
});
