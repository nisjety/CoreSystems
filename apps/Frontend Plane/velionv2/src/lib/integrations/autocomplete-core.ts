import type { RequestActor } from "@/lib/integrations/request-actor";

export type AutocompleteSuggestion = {
  text: string;
  source: string;
  collection: string;
  object: string;
  targetUrl?: string;
  metadata?: unknown;
};

export type AutocompleteSuggestionResponse = {
  configured: boolean;
  query: string;
  scope: string;
  suggestions: AutocompleteSuggestion[];
};

export class AutocompleteCoreError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
  ) {
    super(message);
  }
}

function getAutocompleteCoreUrl() {
  return (process.env.AUTOCOMPLETE_CORE_URL ?? process.env.AUTOCOMPLETE_URL ?? "http://localhost:3219").replace(
    /\/+$/,
    "",
  );
}

function getAutocompleteToken() {
  return process.env.AUTOCOMPLETE_INTERNAL_TOKEN;
}

function resolveOrgId(actor: RequestActor) {
  return process.env.VELION_ORG_ID ?? process.env.DEFAULT_ORG_ID ?? actor.userId;
}

export async function fetchAutocompleteSuggestions(
  actor: RequestActor,
  {
    query,
    scope = "queries",
    limit = 8,
  }: {
    query: string;
    scope?: "all" | "queries" | "hosts" | "titles";
    limit?: number;
  },
): Promise<AutocompleteSuggestionResponse> {
  const trimmed = query.trim();
  const token = getAutocompleteToken();

  if (!token || trimmed.length < 2) {
    return {
      configured: Boolean(token),
      query: trimmed,
      scope,
      suggestions: [],
    };
  }

  const url = new URL("/v1/suggestions", getAutocompleteCoreUrl());
  url.searchParams.set("q", trimmed);
  url.searchParams.set("scope", scope);
  url.searchParams.set("limit", String(Math.max(1, Math.min(limit, 20))));

  const response = await fetch(url, {
    cache: "no-store",
    headers: {
      authorization: `Bearer ${token}`,
      "x-org-id": resolveOrgId(actor),
    },
    signal: AbortSignal.timeout(1500),
  }).catch((error: unknown) => {
    throw new AutocompleteCoreError(
      502,
      "autocomplete_core_unreachable",
      error instanceof Error ? error.message : "autocomplete-core request failed",
    );
  });

  if (!response.ok) {
    const body = await response.json().catch(() => null);
    const message =
      body && typeof body === "object" && "error" in body
        ? JSON.stringify(body.error)
        : `autocomplete-core returned ${response.status}`;
    throw new AutocompleteCoreError(response.status, "autocomplete_core_error", message);
  }

  const body = (await response.json()) as {
    data?: {
      query?: string;
      scope?: string;
      suggestions?: Array<{
        text?: string;
        source?: string;
        collection?: string;
        object?: string;
        target_url?: string;
        targetUrl?: string;
        metadata?: unknown;
      }>;
    };
  };

  const suggestions: AutocompleteSuggestion[] = [];

  for (const suggestion of body.data?.suggestions ?? []) {
    if (typeof suggestion.text !== "string" || suggestion.text.trim().length === 0) continue;

    suggestions.push({
      text: suggestion.text,
      source: suggestion.source ?? "query",
      collection: suggestion.collection ?? scope,
      object: suggestion.object ?? suggestion.text,
      targetUrl: suggestion.targetUrl ?? suggestion.target_url,
      metadata: suggestion.metadata,
    });
  }

  return {
    configured: true,
    query: body.data?.query ?? trimmed,
    scope: body.data?.scope ?? scope,
    suggestions,
  };
}
