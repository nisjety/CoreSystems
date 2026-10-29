import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import ignore from "ignore";
import { describe, expect, it } from "vitest";

describe("Triodelab deployment upload boundary", () => {
  it("keeps the Vercel CLI's root walk traversable instead of recursively excluding every directory", () => {
    const path = fileURLToPath(
      new URL("../../../../.vercelignore", import.meta.url),
    );
    const patterns = readFileSync(path, "utf8")
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter((line) => line && !line.startsWith("#"));
    expect(patterns[0]).toBe("/*");
    expect(patterns).toContain("!frontend");
    expect(patterns).not.toContain("!frontend/");
    expect(patterns).not.toContain("**");
    expect(patterns).not.toContain("!frontend/**");
  });
  it("uploads only the frontend sources and assets, not local secrets or build output", () => {
    const path = fileURLToPath(
      new URL("../../../../.vercelignore", import.meta.url),
    );
    const filter = ignore().add(readFileSync(path, "utf8"));
    for (const file of [
      "backend/.env",
      "backend/server.py",
      ".env",
      ".vercel/auth.json",
      "frontend/.env.production.local",
      "frontend/.env",
      "frontend/src/private/.env.local",
      "frontend/tmp/cert.pem",
      "frontend/config/private.key",
      "frontend/config/private.p12",
      "frontend/config/private.pfx",
      "frontend/tmp/id_rsa",
      "frontend/config/.npmrc",
      "frontend/.local/secrets/token",
      "frontend/.next/server/index.js",
      "frontend/node_modules/private-package/index.js",
      "frontend/.vercel/project.json",
      "frontend/coverage/index.html",
      "frontend/src/nested/.next/build.js",
      "frontend/src/nested/.git/config",
      "frontend/src/nested/.vercel/auth.json",
      "frontend/src/nested/node_modules/package/index.js",
      "frontend/src/nested/.local/secrets/token",
      "frontend/src/nested/coverage/index.html",
      "frontend/tsconfig.tsbuildinfo",
    ]) {
      expect(filter.ignores(file), file).toBe(true);
    }
    for (const file of [
      "frontend/package.json",
      "frontend/pnpm-lock.yaml",
      "frontend/next.config.ts",
      "frontend/src/app/(nb)/page.tsx",
      "frontend/public/favicon.svg",
    ]) {
      expect(filter.ignores(file), file).toBe(false);
    }
  });

  // Opt in explicitly for the installed CLI's real read-only upload walk.
  // It never uploads or creates a deployment. Ordinary unit tests need no login.
  it.skipIf(process.env.TRIODELAB_VERCEL_DRY_RUN !== "1")(
    "verifies the actual Vercel CLI manifest, not only gitignore-library semantics",
    () => {
      const cwd = fileURLToPath(new URL("../../../../", import.meta.url));
      const result = spawnSync(
        "vercel",
        [
          "--scope",
          "team_cW7J2entnq3fYKzQb3UutF9k",
          "deploy",
          "--dry",
          "--format=json",
          "--prod",
          "--project",
          "prj_2XC0iqmcav7TTPSfnzKq4dlgtSuT",
        ],
        { cwd, encoding: "utf8", timeout: 30_000, maxBuffer: 16 * 1024 * 1024 },
      );
      expect(result.status, "read-only Vercel CLI manifest must succeed").toBe(
        0,
      );
      const manifest = JSON.parse(result.stdout) as {
        fileCount: number;
        files: Array<string | { path?: string; name?: string; file?: string }>;
      };
      const paths = manifest.files.map((entry) =>
        typeof entry === "string"
          ? entry
          : (entry.path ?? entry.name ?? entry.file ?? ""),
      );
      expect(manifest.fileCount).toBeGreaterThan(0);
      for (const required of [
        "frontend/package.json",
        "frontend/package-lock.json",
        "frontend/pnpm-lock.yaml",
        "frontend/next.config.ts",
        "frontend/src/app/(nb)/page.tsx",
        "frontend/src/app/(en)/en/page.tsx",
        "frontend/src/components/layout/root-document.tsx",
        "frontend/public/favicon.svg",
      ])
        expect(paths, required).toContain(required);
      expect(paths.every((path) => path.startsWith("frontend/"))).toBe(true);
      const forbidden =
        /(^|\/)(?:\.env[^/]*|\.git|\.vercel|\.next|node_modules|\.local|\.fallow|coverage|test-results|playwright-report|\.npmrc|\.pypirc|\.netrc|id_rsa|id_ecdsa|id_ed25519)(?:\/|$)|\.(?:pem|key|p12|pfx|keystore|tsbuildinfo)$/i;
      expect(paths.filter((path) => forbidden.test(path))).toEqual([]);
    },
    35_000,
  );
});
