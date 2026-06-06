import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import {
  slugify,
  sizeFromEmployeeCount,
  formatBrregAddress,
  brregService,
} from "@/lib/services/brreg-service";

// ---------------------------------------------------------------------------
// slugify
// ---------------------------------------------------------------------------
describe("slugify", () => {
  it("lowercases input", () => {
    expect(slugify("Hello World")).toBe("hello-world");
  });

  it("replaces Norwegian æ with ae", () => {
    expect(slugify("Ærlig")).toBe("aerlig");
  });

  it("replaces Norwegian ø with o", () => {
    expect(slugify("Øst")).toBe("ost");
  });

  it("replaces Norwegian å with a", () => {
    expect(slugify("Ålesund")).toBe("alesund");
  });

  it("strips punctuation and collapses to dashes", () => {
    expect(slugify("Hello, World!")).toBe("hello-world");
  });

  it("trims leading and trailing dashes", () => {
    expect(slugify("  -Hello-  ")).toBe("hello");
  });

  it("truncates to 48 characters", () => {
    const long = "a".repeat(60);
    expect(slugify(long).length).toBe(48);
  });

  it("handles a realistic Norwegian org name", () => {
    expect(slugify("Møller & Sønner AS")).toBe("moller-sonner-as");
  });
});

// ---------------------------------------------------------------------------
// sizeFromEmployeeCount
// ---------------------------------------------------------------------------
describe("sizeFromEmployeeCount", () => {
  it("returns '1' for undefined", () => {
    expect(sizeFromEmployeeCount(undefined)).toBe("1");
  });

  it("returns '1' for 0", () => {
    expect(sizeFromEmployeeCount(0)).toBe("1");
  });

  it("returns '1' for 1", () => {
    expect(sizeFromEmployeeCount(1)).toBe("1");
  });

  it("returns '2-10' for 10 (upper boundary)", () => {
    expect(sizeFromEmployeeCount(10)).toBe("2-10");
  });

  it("returns '11-50' for 11 (lower boundary)", () => {
    expect(sizeFromEmployeeCount(11)).toBe("11-50");
  });

  it("returns '11-50' for 50 (upper boundary)", () => {
    expect(sizeFromEmployeeCount(50)).toBe("11-50");
  });

  it("returns '51-200' for 51 (lower boundary)", () => {
    expect(sizeFromEmployeeCount(51)).toBe("51-200");
  });

  it("returns '51-200' for 200 (upper boundary)", () => {
    expect(sizeFromEmployeeCount(200)).toBe("51-200");
  });

  it("returns '201-1000' for 201 (lower boundary)", () => {
    expect(sizeFromEmployeeCount(201)).toBe("201-1000");
  });

  it("returns '201-1000' for 1000 (upper boundary)", () => {
    expect(sizeFromEmployeeCount(1000)).toBe("201-1000");
  });

  it("returns '1000+' for 1001", () => {
    expect(sizeFromEmployeeCount(1001)).toBe("1000+");
  });
});

// ---------------------------------------------------------------------------
// formatBrregAddress
// ---------------------------------------------------------------------------
describe("formatBrregAddress", () => {
  it("returns empty string for undefined", () => {
    expect(formatBrregAddress(undefined)).toBe("");
  });

  it("composes adresse lines with postnummer and poststed", () => {
    expect(
      formatBrregAddress({
        adresse: ["Storgata 1"],
        postnummer: "0182",
        poststed: "Oslo",
      }),
    ).toBe("Storgata 1, 0182 Oslo");
  });

  it("handles multiple adresse lines", () => {
    expect(
      formatBrregAddress({
        adresse: ["Bygg A", "Storgata 1"],
        postnummer: "0182",
        poststed: "Oslo",
      }),
    ).toBe("Bygg A, Storgata 1, 0182 Oslo");
  });

  it("handles missing postnummer gracefully", () => {
    expect(
      formatBrregAddress({
        adresse: ["Storgata 1"],
        poststed: "Oslo",
      }),
    ).toBe("Storgata 1, Oslo");
  });

  it("handles missing adresse array gracefully", () => {
    expect(
      formatBrregAddress({
        postnummer: "0182",
        poststed: "Oslo",
      }),
    ).toBe("0182 Oslo");
  });

  it("handles completely empty address object", () => {
    expect(formatBrregAddress({})).toBe("");
  });
});

// ---------------------------------------------------------------------------
// brregService — fetch mocks
// ---------------------------------------------------------------------------

function makeFetchResponse(
  body: unknown,
  status = 200,
): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: () => Promise.resolve(body),
  } as Response;
}

describe("brregService.searchByName", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("calls the correct URL with query and size", async () => {
    const mockFetch = vi.mocked(fetch);
    mockFetch.mockResolvedValueOnce(
      makeFetchResponse({ results: [], count: 0 }),
    );

    await brregService.searchByName("Acme", 5);

    expect(mockFetch).toHaveBeenCalledOnce();
    const url = mockFetch.mock.calls[0][0] as string;
    expect(url).toBe("/api/org/api/v1/brreg/search?q=Acme&size=5");
  });

  it("uses default size of 10", async () => {
    const mockFetch = vi.mocked(fetch);
    mockFetch.mockResolvedValueOnce(
      makeFetchResponse({ results: [], count: 0 }),
    );

    await brregService.searchByName("Test");

    const url = mockFetch.mock.calls[0][0] as string;
    expect(url).toContain("size=10");
  });

  it("returns the results array", async () => {
    const mockFetch = vi.mocked(fetch);
    const entity = { organisasjonsnummer: "123456789", navn: "Acme AS" };
    mockFetch.mockResolvedValueOnce(
      makeFetchResponse({ results: [entity], count: 1 }),
    );

    const result = await brregService.searchByName("Acme");
    expect(result).toEqual([entity]);
  });

  it("returns an empty array when upstream results are null", async () => {
    const mockFetch = vi.mocked(fetch);
    mockFetch.mockResolvedValueOnce(
      makeFetchResponse({ results: null, count: 0 }),
    );

    const result = await brregService.searchByName("Acme");
    expect(result).toEqual([]);
  });

  it("returns an empty array when upstream results are missing", async () => {
    const mockFetch = vi.mocked(fetch);
    mockFetch.mockResolvedValueOnce(makeFetchResponse({ count: 0 }));

    const result = await brregService.searchByName("Acme");
    expect(result).toEqual([]);
  });

  it("accepts a direct array response for compatibility", async () => {
    const mockFetch = vi.mocked(fetch);
    const entity = { organisasjonsnummer: "123456789", navn: "Acme AS" };
    mockFetch.mockResolvedValueOnce(makeFetchResponse([entity]));

    const result = await brregService.searchByName("Acme");
    expect(result).toEqual([entity]);
  });

  it("throws when response is not ok", async () => {
    const mockFetch = vi.mocked(fetch);
    mockFetch.mockResolvedValueOnce(
      makeFetchResponse({ error: "Service unavailable" }, 503),
    );

    await expect(brregService.searchByName("Acme")).rejects.toThrow(
      "Service unavailable",
    );
  });
});

describe("brregService.lookupByOrgNr", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("calls the correct URL", async () => {
    const mockFetch = vi.mocked(fetch);
    const entity = { organisasjonsnummer: "987654321", navn: "Test AS" };
    mockFetch.mockResolvedValueOnce(makeFetchResponse(entity));

    await brregService.lookupByOrgNr("987654321");

    const url = mockFetch.mock.calls[0][0] as string;
    expect(url).toBe("/api/org/api/v1/brreg/987654321");
  });

  it("returns null for 404", async () => {
    const mockFetch = vi.mocked(fetch);
    mockFetch.mockResolvedValueOnce(makeFetchResponse({}, 404));

    const result = await brregService.lookupByOrgNr("000000000");
    expect(result).toBeNull();
  });

  it("throws on non-ok, non-404 response", async () => {
    const mockFetch = vi.mocked(fetch);
    mockFetch.mockResolvedValueOnce(
      makeFetchResponse({ error: "Bad gateway" }, 502),
    );

    await expect(brregService.lookupByOrgNr("123456789")).rejects.toThrow(
      "Bad gateway",
    );
  });

  it("returns the entity on 200", async () => {
    const mockFetch = vi.mocked(fetch);
    const entity = { organisasjonsnummer: "987654321", navn: "Test AS", konkurs: false, underAvvikling: false };
    mockFetch.mockResolvedValueOnce(makeFetchResponse(entity));

    const result = await brregService.lookupByOrgNr("987654321");
    expect(result).toEqual(entity);
  });
});
