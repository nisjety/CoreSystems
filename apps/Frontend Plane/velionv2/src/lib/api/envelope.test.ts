import { describe, expect, it } from "vitest";
import { apiErrorSchema, cursorMetaSchema, cursorPage, fail, ok } from "@/lib/api/envelope";

describe("api envelope", () => {
  it("wraps successful resources in a data envelope", () => {
    expect(ok({ id: "org_1" })).toEqual({ data: { id: "org_1" } });
  });

  it("creates cursor metadata and next links for paginated collections", () => {
    const page = cursorPage({
      data: [{ id: "conv_1" }],
      limit: 1,
      nextCursor: "1",
      self: "/api/v1/conversations?limit=1",
    });

    expect(cursorMetaSchema.parse(page.meta)).toEqual({
      limit: 1,
      hasNext: true,
      nextCursor: "1",
    });
    expect(page.links?.next).toBe("/api/v1/conversations?limit=1&cursor=1");
  });

  it("creates typed API errors with field details", () => {
    const error = fail({
      code: "validation_error",
      message: "Request validation failed",
      details: [
        {
          field: "limit",
          code: "out_of_range",
          message: "Use a value from 1 to 100.",
        },
      ],
    });

    expect(apiErrorSchema.parse(error).error.details?.[0]?.field).toBe("limit");
  });
});
