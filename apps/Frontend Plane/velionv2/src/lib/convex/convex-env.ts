function readPublicEnv(name: string): string | null {
  const value = process.env[name]?.trim();
  return value ? value.replace(/\/+$/, "") : null;
}

export function getPublicConvexUrl(): string | null {
  return (
    readPublicEnv("NEXT_PUBLIC_CONVEX_URL") ??
    readPublicEnv("NEXT_PUBLIC_CONVEX_HTTP")
  );
}
