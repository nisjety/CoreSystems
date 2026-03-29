// Search is handled server-side via /api/ai/search proxy route.
// No client-side URL needed — all calls are same-origin relative.

export interface SearchSource {
  id: string;
  title: string;
  url: string;
  snippet: string;
  type: 'website' | 'document' | 'sharepoint' | 'teams' | 'email';
  updatedAt?: string;
  score?: number;
}

export interface SearchResult {
  answer: string;
  sources: SearchSource[];
  query: string;
  duration?: number;
}

export interface SearchStreamChunk {
  type: 'answer_chunk' | 'sources' | 'done' | 'error';
  content?: string;
  sources?: SearchSource[];
  error?: string;
}

/**
 * Stream an org-knowledge search answer. Yields chunks as they arrive.
 * Falls back to a mock response if the AI core is unavailable.
 */
export async function* streamSearch(
  query: string,
  signal?: AbortSignal,
): AsyncGenerator<SearchStreamChunk> {
  try {
    const res = await fetch('/api/ai/search', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ query, limit: 8 }),
      signal,
    });

    if (!res.ok || !res.body) throw new Error(`HTTP ${res.status}`);

    const reader = res.body.getReader();
    const decoder = new TextDecoder();
    let buf = '';

    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      buf += decoder.decode(value, { stream: true });

      const lines = buf.split('\n');
      buf = lines.pop() ?? '';

      for (const line of lines) {
        if (!line.startsWith('data: ')) continue;
        const raw = line.slice(6).trim();
        if (!raw || raw === '[DONE]') continue;
        try {
          yield JSON.parse(raw) as SearchStreamChunk;
        } catch {
          // skip malformed chunks
        }
      }
    }

    yield { type: 'done' };
  } catch (err: unknown) {
    if (err instanceof Error && err.name === 'AbortError') return;
    // Dev fallback — mock response
    yield* mockSearch(query);
  }
}

async function* mockSearch(query: string): AsyncGenerator<SearchStreamChunk> {
  const answer = `Dette er et simulert svar på spørsmålet ditt: «${query}».\n\nI et produksjonsmiljø ville AI-kjernen (reasoning-plane) ha returnert et svar basert på organisasjonens indekserte innhold — nettsider, dokumenter og SharePoint-biblioteker.\n\nKoble til en datakilde under Innstillinger → Integrasjoner for å begynne.`;
  const words = answer.split(' ');

  for (let i = 0; i < words.length; i++) {
    await new Promise((r) => setTimeout(r, 28));
    yield { type: 'answer_chunk', content: (i === 0 ? '' : ' ') + words[i] };
  }

  yield {
    type: 'sources',
    sources: [
      {
        id: '1',
        title: 'Eksempel: Selskapets nettside',
        url: 'https://eksempel.no/om-oss',
        snippet: 'Om oss — vi er et selskap som jobber med …',
        type: 'website',
        updatedAt: new Date().toISOString(),
        score: 0.94,
      },
      {
        id: '2',
        title: 'Eksempel: Prosedyredokument',
        url: 'https://eksempel.sharepoint.com/docs/prosedyre.pdf',
        snippet: 'Interne prosedyrer for …',
        type: 'sharepoint',
        updatedAt: new Date().toISOString(),
        score: 0.87,
      },
    ],
  };

  yield { type: 'done' };
}
