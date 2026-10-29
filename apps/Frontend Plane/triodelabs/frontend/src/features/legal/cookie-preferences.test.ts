import { describe, expect, it, vi } from "vitest";
import { openCookiePreferences } from "./cookie-preferences";

describe("cookie preference control", () => {
  it("opens the existing consent manager without granting any purpose", () => {
    const renew = vi.fn();
    expect(openCookiePreferences({ renew })).toBe(true);
    expect(renew).toHaveBeenCalledExactlyOnceWith();
  });
  it("fails visibly when the consent manager is unavailable", () => {
    expect(openCookiePreferences()).toBe(false);
    expect(openCookiePreferences({})).toBe(false);
    expect(
      openCookiePreferences({
        renew: () => {
          throw new Error("CMP unavailable");
        },
      }),
    ).toBe(false);
  });
});
