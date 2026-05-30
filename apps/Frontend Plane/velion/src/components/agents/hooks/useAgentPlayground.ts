'use client';

import { useCallback, useRef, useState } from 'react';

import { useRunEvents, type RunEventsState } from '@/lib/hooks/useRunEvents';

import { resolveAgentProfile, type AgentProfile, type PersistedAgent } from '../types';

/**
 * U3-6 (ui-ux-velion-gap.md §14): playground chat hook.
 *
 * Drives the right-pane "Test your agent" surface in `AgentWorkspaceView`.
 * Sends the message to the existing `/api/chat/stream` endpoint with the
 * agent's `agentId` set so the gateway picks up the agent's `systemPrompt`,
 * `model`, and `temperature` from Convex on the server side.
 *
 * Why reuse `/api/chat/stream` instead of building a dedicated playground
 * endpoint:
 *   - the same code path handles auth, JWT minting, session creation,
 *     Convex persistence, and the Model Plane invocation
 *   - playground runs land in `agentRuns` (via orchestrator-core's events)
 *     so the analytics tab populates naturally
 *   - tool-call loop, browse-web, deep-research are all available because
 *     the agent's tools list flows through `agentConfig.tools` already
 *
 * The hook keeps an in-memory message list per session. Refreshing the
 * page resets the playground — that's fine: playground conversations are
 * ephemeral by design, real chats live under `/chat`.
 *
 * `browseWeb` defaults to true so the agent can ground answers when its
 * tools list includes web access. Override per-call when the operator
 * wants to test the unaugmented base behaviour.
 */
export type PlaygroundRole = 'user' | 'assistant' | 'system';

export interface PlaygroundMessage {
  id: string;
  role: PlaygroundRole;
  content: string;
  timestamp: number;
  /** True while the assistant message is still streaming chunks. */
  streaming?: boolean;
  /** Populated when the assistant call failed. */
  error?: string;
  /**
   * Wave 11 §5 — agent-run identifier returned by the gateway on the
   * final SSE envelope. The Fin G/A/P rating chips POST to
   * `/api/agents/runs/{runId}/rate` to persist operator feedback.
   * Absent on the welcome message and on errored turns.
   */
  runId?: string;
  /**
   * Wave 11 §5 — operator's rating for this assistant turn. Mirrored
   * from `agentRuns.rating` in Convex. Set optimistically when the
   * operator clicks a chip; reverts if the POST fails.
   */
  rating?: 'good' | 'acceptable' | 'poor';
  /**
   * Wave 9 §19: per-turn tool-loop trace. One entry per tool execution
   * during this turn. Populated when the agent had tools enabled AND
   * the model invoked them — empty for plain single-turn replies.
   */
  toolTrace?: Array<{
    round: number;
    tool: string;
    args_preview: string;
    result_preview: string;
    result_bytes: number;
  }>;
}

interface UsePlaygroundOptions {
  /** The persisted agent record. The hook is a no-op when `null`. */
  agent: PersistedAgent | null;
  /** Override the agent's stored greeting (defaults to "Hi! …" when blank). */
  greeting?: string;
  /** When true, browse-web grounding is requested for every turn. */
  browseWeb?: boolean;
}

interface UsePlaygroundReturn {
  messages: PlaygroundMessage[];
  isStreaming: boolean;
  send: (content: string) => Promise<void>;
  reset: () => void;
  /**
   * Wave 11 §5 — set the operator's rating for an assistant turn.
   * Optimistic update on success; reverts on POST failure. Returns
   * true if the persisted rating was accepted.
   */
  rateMessage: (messageId: string, rating: 'good' | 'acceptable' | 'poor') => Promise<boolean>;
  /**
   * Run id of the most recent assistant turn (set on the final SSE
   * envelope). `null` until the first turn completes.
   */
  activeRunId: string | null;
  /**
   * Live task-graph state for {@link activeRunId} via the shared
   * `useRunEvents` subscription — plan/todo/approval transitions plus
   * pause-for-approval (HITL). Lets the operator watch the harness work
   * during a test run instead of each surface reinventing polling.
   */
  runEvents: RunEventsState;
  /**
   * Resolved harness profile for the agent. The view shows operator surfaces
   * (run-event panel, HITL) only for `deployed_agent`; `chat` stays clean.
   */
  profile: AgentProfile;
}

function nowId(): string {
  return `m-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

interface SseEvent {
  type?: string;
  content?: string;
  error?: string;
  /**
   * Wave 11 §5 — the gateway's run id, forwarded by the chat-stream
   * route on the final message envelope so the playground knows which
   * `agentRuns` row to attach a rating to.
   */
  runId?: string;
  /**
   * Wave 9 §19: the chat-stream route forwards the gateway's
   * `tool_trace` array on the final message envelope. Each entry maps
   * 1:1 to a tool execution during the turn.
   */
  metadata?: {
    runId?: string;
    toolTrace?: Array<{
      round: number;
      tool: string;
      args_preview: string;
      result_preview: string;
      result_bytes: number;
    }>;
  };
}

function readSseEvent(line: string): SseEvent | null {
  if (!line.startsWith('data: ')) return null;
  const raw = line.slice(6).trim();
  if (!raw) return null;
  try {
    return JSON.parse(raw) as SseEvent;
  } catch {
    return null;
  }
}

export function useAgentPlayground({
  agent,
  greeting,
  browseWeb = true,
}: UsePlaygroundOptions): UsePlaygroundReturn {
  const [messages, setMessages] = useState<PlaygroundMessage[]>(() => {
    const opener = greeting ?? agent?.greeting ?? 'Hi! How can I help you today?';
    if (!opener.trim()) return [];
    return [
      {
        id: nowId(),
        role: 'assistant',
        content: opener,
        timestamp: Date.now(),
      },
    ];
  });
  const [isStreaming, setIsStreaming] = useState<boolean>(false);
  // Run id of the latest completed turn — feeds the shared run-event stream.
  const [activeRunId, setActiveRunId] = useState<string | null>(null);
  const runEvents = useRunEvents(activeRunId);
  // Per-mount session id so /api/chat/stream reuses the same Convex
  // conversation across turns in the playground.
  const sessionIdRef = useRef<string>(`playground-${agent?.id ?? 'anon'}-${Date.now()}`);

  const reset = useCallback((): void => {
    sessionIdRef.current = `playground-${agent?.id ?? 'anon'}-${Date.now()}`;
    const opener = greeting ?? agent?.greeting ?? 'Hi! How can I help you today?';
    setMessages(
      opener.trim()
        ? [{ id: nowId(), role: 'assistant', content: opener, timestamp: Date.now() }]
        : [],
    );
    setIsStreaming(false);
    setActiveRunId(null);
  }, [agent?.id, agent?.greeting, greeting]);

  const send = useCallback(
    async (content: string): Promise<void> => {
      const trimmed = content.trim();
      if (!trimmed || !agent || isStreaming) return;

      const userMsg: PlaygroundMessage = {
        id: nowId(),
        role: 'user',
        content: trimmed,
        timestamp: Date.now(),
      };
      const placeholderId = nowId();
      setMessages((prev) => [
        ...prev,
        userMsg,
        {
          id: placeholderId,
          role: 'assistant',
          content: '',
          timestamp: Date.now(),
          streaming: true,
        },
      ]);
      setIsStreaming(true);

      try {
        const res = await fetch('/api/chat/stream', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            sessionId: sessionIdRef.current,
            content: trimmed,
            agentId: agent.id,
            model: agent.model,
            browseWeb,
          }),
        });

        if (!res.ok || !res.body) {
          throw new Error(`HTTP ${res.status}`);
        }

        // Stream + collate. The route emits `data: <json>\n\n` lines with
        // payload shapes from session-store: each emitted assistant
        // message contains the latest accumulated `content` snapshot, so
        // we just replace the placeholder content on every chunk.
        const reader = res.body.getReader();
        const decoder = new TextDecoder();
        let buffer = '';
        let latestContent = '';

        let latestTrace: PlaygroundMessage['toolTrace'];
        let latestRunId: string | undefined;
        while (true) {
          const { value, done } = await reader.read();
          if (done) break;
          buffer += decoder.decode(value, { stream: true });

          let nl = buffer.indexOf('\n\n');
          while (nl !== -1) {
            const event = readSseEvent(buffer.slice(0, nl));
            buffer = buffer.slice(nl + 2);
            nl = buffer.indexOf('\n\n');

            if (!event) continue;
            // session-store payloads carry `content`; SSE error shapes
            // carry `error`. Wave 9 adds optional `metadata.toolTrace`
            // and Wave 11 adds `metadata.runId` (or top-level `runId`)
            // on the final envelope — capture both for the UI.
            if (event.metadata?.toolTrace) {
              latestTrace = event.metadata.toolTrace;
            }
            if (typeof event.metadata?.runId === 'string') {
              latestRunId = event.metadata.runId;
            } else if (typeof event.runId === 'string') {
              latestRunId = event.runId;
            }
            if (typeof event.content === 'string') {
              latestContent = event.content;
              setMessages((prev) =>
                prev.map((m) =>
                  m.id === placeholderId
                    ? { ...m, content: latestContent, streaming: true }
                    : m,
                ),
              );
            }
          }
        }

        setMessages((prev) =>
          prev.map((m) =>
            m.id === placeholderId
              ? {
                  ...m,
                  streaming: false,
                  content: latestContent || m.content,
                  toolTrace: latestTrace,
                  runId: latestRunId,
                }
              : m,
          ),
        );

        // Point the shared run-event stream at this turn so the operator can
        // inspect the run's plan/approval/pause activity.
        if (latestRunId) {
          setActiveRunId(latestRunId);
        }
      } catch (err: unknown) {
        const detail = err instanceof Error ? err.message : 'unknown error';
        setMessages((prev) =>
          prev.map((m) =>
            m.id === placeholderId
              ? {
                  ...m,
                  streaming: false,
                  content: 'Sorry — the agent could not respond.',
                  error: detail,
                }
              : m,
          ),
        );
      } finally {
        setIsStreaming(false);
      }
    },
    [agent, browseWeb, isStreaming],
  );

  const rateMessage = useCallback(
    async (
      messageId: string,
      rating: 'good' | 'acceptable' | 'poor',
    ): Promise<boolean> => {
      const target = messages.find((m) => m.id === messageId);
      if (!target || !target.runId) return false;
      const previous = target.rating;

      // Optimistic update — operator gets instant feedback.
      setMessages((prev) =>
        prev.map((m) => (m.id === messageId ? { ...m, rating } : m)),
      );

      try {
        const response = await fetch(
          `/api/agents/runs/${encodeURIComponent(target.runId)}/rate`,
          {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ rating }),
          },
        );
        if (!response.ok) {
          throw new Error(`HTTP ${response.status}`);
        }
        return true;
      } catch {
        // Revert on failure so the operator can retry.
        setMessages((prev) =>
          prev.map((m) =>
            m.id === messageId ? { ...m, rating: previous } : m,
          ),
        );
        return false;
      }
    },
    [messages],
  );

  const profile = resolveAgentProfile(agent);

  return { messages, isStreaming, send, reset, rateMessage, activeRunId, runEvents, profile };
}
