import { jsonOrNull, ZAMMAD_URL, zammadConfigured, zammadHeaders } from "@/app/api/support/_lib/zammad";

type RuntimeResource = "agents" | "groups" | "macros";

type RuntimeStatus = {
  support: {
    configured: boolean;
    connected: boolean;
    agents: number;
    groups: number;
    macros: number;
    message: string;
  };
};

const supportEndpoints: Array<{ key: RuntimeResource; url: string }> = [
  { key: "agents", url: `${ZAMMAD_URL}/api/v1/users?role=Agent` },
  { key: "groups", url: `${ZAMMAD_URL}/api/v1/groups` },
  { key: "macros", url: `${ZAMMAD_URL}/api/v1/macros` },
];

export async function GET() {
  if (!zammadConfigured()) {
    return Response.json({
      support: {
        configured: false,
        connected: false,
        agents: 0,
        groups: 0,
        macros: 0,
        message: "Set ZAMMAD_API_URL and ZAMMAD_API_TOKEN to enable live support actions.",
      },
    } satisfies RuntimeStatus);
  }

  const results = await Promise.allSettled(
    supportEndpoints.map(async ({ key, url }) => {
      const response = await fetch(url, {
        headers: zammadHeaders(),
        cache: "no-store",
      });
      const payload = await jsonOrNull<unknown>(response);

      if (!response.ok) {
        throw new Error(`${key}_fetch_failed`);
      }

      return { key, count: countRuntimePayload(payload) };
    }),
  );

  const counts = results.reduce(
    (next, result) => {
      if (result.status === "fulfilled") {
        next[result.value.key] = result.value.count;
      }

      return next;
    },
    { agents: 0, groups: 0, macros: 0 },
  );
  const connected = results.some((result) => result.status === "fulfilled");

  return Response.json({
    support: {
      configured: true,
      connected,
      ...counts,
      message: connected
        ? "Support integration connected. Agent actions can use live support teams, groups, and macros."
        : "Support integration is configured, but Velion could not reach Zammad.",
    },
  } satisfies RuntimeStatus);
}

function countRuntimePayload(payload: unknown) {
  if (Array.isArray(payload)) {
    return payload.length;
  }

  if (payload && typeof payload === "object") {
    return Object.keys(payload).length;
  }

  return 0;
}
