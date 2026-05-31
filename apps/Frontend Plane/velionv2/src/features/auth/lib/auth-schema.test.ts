import { describe, expect, it } from "vitest";
import {
  calculatePasswordStrength,
  signInSchema,
  signUpSchema,
} from "@/features/auth/lib/auth-schema";

describe("auth schema", () => {
  it("accepts a valid sign-in request", () => {
    const parsed = signInSchema.safeParse({
      email: "security@example.com",
      password: "CorrectHorse1!",
    });

    expect(parsed.success).toBe(true);
  });

  it("rejects mismatched signup passwords", () => {
    const parsed = signUpSchema.safeParse({
      name: "Alex Smith",
      email: "alex@example.com",
      password: "CorrectHorse1!",
      confirmPassword: "WrongHorse1!",
    });

    expect(parsed.success).toBe(false);
  });

  it("requires Better Auth's configured 12 character signup minimum", () => {
    const parsed = signUpSchema.safeParse({
      name: "Alex Smith",
      email: "alex@example.com",
      password: "Short1!",
      confirmPassword: "Short1!",
    });

    expect(parsed.success).toBe(false);
  });

  it("scores strong passwords higher than weak passwords", () => {
    expect(calculatePasswordStrength("password").score).toBeLessThan(
      calculatePasswordStrength("CorrectHorse1!").score,
    );
  });
});
