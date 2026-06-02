import { describe, expect, it } from "vitest"
import {
  buildGroundingContent,
  dedupeSources,
  type GroundingInput,
  type ThreadTurn,
} from "./answer-thread"

describe("dedupeSources", () => {
  it("removes duplicate urls preserving first-seen order", () => {
    const out = dedupeSources([
      { url: "https://a.com", title: "A" },
      { url: "https://b.com", title: "B" },
      { url: "https://a.com", title: "A2" },
    ])
    expect(out.map((s) => s.url)).toEqual(["https://a.com", "https://b.com"])
    expect(out[0]?.title).toBe("A") // first wins
  })

  it("skips blank/whitespace urls", () => {
    const out = dedupeSources([
      { url: "  " },
      { url: "" },
      { url: "https://x.com" },
    ])
    expect(out).toHaveLength(1)
    expect(out[0]?.url).toBe("https://x.com")
  })

  it("caps the count at 6", () => {
    const many = Array.from({ length: 12 }, (_, i) => ({ url: `https://s${i}.com` }))
    expect(dedupeSources(many)).toHaveLength(6)
  })
})

describe("buildGroundingContent", () => {
  const base: GroundingInput = {
    query: "spa berlin",
    answer: "Et kort sammendrag.",
    sources: [{ url: "https://hotel.com", title: "Hotel" }],
    priorTurns: [],
    question: "har de basseng?",
  }

  it("includes query, summary, sources and the follow-up question", () => {
    const c = buildGroundingContent(base)
    expect(c).toContain("Søk: spa berlin")
    expect(c).toContain("Sammendrag: Et kort sammendrag.")
    expect(c).toContain("Kilder:")
    expect(c).toContain("- Hotel https://hotel.com")
    expect(c).toContain("Oppfølging: har de basseng?")
  })

  it("omits Sammendrag when the answer is empty and Kilder when no sources", () => {
    const c = buildGroundingContent({ ...base, answer: "", sources: [] })
    expect(c).not.toContain("Sammendrag:")
    expect(c).not.toContain("Kilder:")
    expect(c).toContain("Søk: spa berlin")
  })

  it("renders prior turns with role labels, keeping only the last 8", () => {
    const priorTurns: ThreadTurn[] = Array.from({ length: 10 }, (_, i) => ({
      id: String(i),
      role: i % 2 === 0 ? "user" : "assistant",
      text: `turn ${i}`,
    }))
    const c = buildGroundingContent({ ...base, priorTurns })
    expect(c).toContain("Tidligere:")
    expect(c).toContain("turn 9")
    expect(c).not.toContain("turn 0") // dropped — only last 8 kept
    expect(c).toMatch(/Spørsmål:|Svar:/)
  })

  it("truncates an over-long answer with an ellipsis", () => {
    const c = buildGroundingContent({ ...base, answer: "x".repeat(2000) })
    const line = c.split("\n").find((l) => l.startsWith("Sammendrag:"))!
    expect(line.length).toBeLessThan(1300)
    expect(line.endsWith("…")).toBe(true)
  })
})
