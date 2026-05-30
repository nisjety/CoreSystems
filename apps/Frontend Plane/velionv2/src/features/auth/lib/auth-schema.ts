import { z } from "zod";

export type AuthMode = "signin" | "signup";

export const signInSchema = z.object({
  email: z.string().trim().email("Enter a valid email address").max(254),
  password: z.string().min(1, "Password is required"),
});

const signUpShape = {
  email: signInSchema.shape.email,
  password: z.string().min(12, "Password must be at least 12 characters"),
  name: z.string().trim().min(2, "Enter your full name").max(96),
  confirmPassword: z.string().min(12, "Confirm your password"),
};

export const signUpSchema = z.object(signUpShape).refine((value) => value.password === value.confirmPassword, {
  path: ["confirmPassword"],
  message: "Passwords must match",
});

export type SignInInput = z.infer<typeof signInSchema>;
export type SignUpInput = z.infer<typeof signUpSchema>;

export type PasswordStrengthKey =
  | "minLength"
  | "uppercase"
  | "lowercase"
  | "number"
  | "special"
  | "long";

export interface PasswordStrength {
  score: number;
  missing: PasswordStrengthKey[];
}

export function calculatePasswordStrength(password: string): PasswordStrength {
  const checks: Array<[PasswordStrengthKey, boolean]> = [
    ["minLength", password.length >= 12],
    ["uppercase", /[A-Z]/.test(password)],
    ["lowercase", /[a-z]/.test(password)],
    ["number", /\d/.test(password)],
    ["special", /[^A-Za-z0-9]/.test(password)],
    ["long", password.length >= 16],
  ];

  const missing = checks.reduce<PasswordStrengthKey[]>(
    (keys, [key, passes]) => (passes ? keys : [...keys, key]),
    [],
  );

  return {
    score: checks.length - missing.length,
    missing,
  };
}

export function authModeTitle(mode: AuthMode) {
  return mode === "signup" ? "Registrer" : "Logg Inn";
}

export function authModeDescription(mode: AuthMode) {
  return mode === "signup"
    ? "Opprett en konto for å starte trygt."
    : "Velkommen tilbake! Logg inn på kontoen din.";
}
