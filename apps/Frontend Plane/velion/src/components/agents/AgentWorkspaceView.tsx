'use client';

import { useState, useCallback, type ReactElement } from 'react';
import { ArrowUpRight, ChevronDown, ChevronRight, Mic, RefreshCcw, Send, Sparkles, X, Settings, Database, Activity, Code, Check, Loader2, Calendar, Workflow, GraduationCap, Share2, Copy, Inbox } from 'lucide-react';
import { type AgentWithMeta } from './data';
import { resolveAgentProfile, type PersistedAgent, type SupportedModel } from './types';
import { AgentInbox } from './AgentInbox';
import { SUPPORTED_MODELS, DEFAULT_MODEL } from '@/app/api/chat/_lib/models';
import { useAgentStats } from './hooks/useAgentStats';
import { useAgentPlayground, type PlaygroundMessage } from './hooks/useAgentPlayground';
import type { RunEventsState } from '@/lib/hooks/useRunEvents';
import { useAgentTools } from './hooks/useAgentTools';
import { useAgentKnowledge } from './hooks/useAgentKnowledge';
import { useAgentCron, type CronEntry } from './hooks/useAgentCron';
import { useAgentFinetune, type FinetuneJob } from './hooks/useAgentFinetune';
import { useAgentEmbed } from './hooks/useAgentEmbed';
import { PlaygroundRatingChips } from './PlaygroundRatingChips';

type AgentViewInput = AgentWithMeta | (PersistedAgent & { greeting?: string })

function isPersistedAgent(agent: AgentViewInput): agent is PersistedAgent {
  return 'orgId' in agent
}

type SaveStatus = 'idle' | 'saving' | 'saved' | 'error'

type WorkspaceTab =
  | 'playground'
  | 'inbox'
  | 'knowledge'
  | 'actions'
  | 'analytics'
  | 'schedules'
  | 'finetune'
  | 'embed';

/**
 * U3-5 + U3-12 (ui-ux-velion-gap.md §14): map the URL slug to a workspace tab.
 *
 * The route is `/agents/{agentId}/{viewId}` with `viewId` coming from the
 * catch-all slug. The set of valid slugs is intentionally wider than the
 * tabs we render so the same URLs the sidebar uses also work as direct
 * deep-links. When the slug doesn't map cleanly, we default to `playground`.
 *
 *   playground / test / settings → playground
 *   knowledge  / train           → knowledge
 *   actions    / deploy          → actions
 *   workflows  / automations     → schedules  (per-agent cron — U3-12)
 *   analyze    / changelog       → analytics
 */
function viewIdToTab(viewId: string | undefined): WorkspaceTab {
  switch (viewId) {
    case 'playground':
    case 'test':
    case 'settings':
      return 'playground';
    case 'knowledge':
    case 'train':
      return 'knowledge';
    case 'actions':
      return 'actions';
    case 'workflows':
    case 'automations':
      return 'schedules';
    case 'inbox':
    case 'conversations':
      return 'inbox';
    case 'analyze':
    case 'changelog':
      return 'analytics';
    case 'embed':
    case 'deploy':
      // `/agents/{id}/deploy` was previously mapped to `actions`. Wave 9
      // makes embed the natural deploy surface — script tag → live
      // bubble on a customer site. Existing deep links keep working.
      return 'embed';
    case 'finetune':
    case 'fine-tune':
    case 'train':
      // Wave 7: `train` semantically overlaps with both knowledge
      // (upload training docs) AND fine-tune (gradient training on a
      // dataset). Fine-tune is the closer match for the literal slug.
      // U3-5 mapped `train` → knowledge; we keep that for back-compat
      // but `finetune` / `fine-tune` route directly here.
      return 'finetune';
    default:
      return 'playground';
  }
}

export function AgentWorkspaceView({
  agent,
  viewId,
}: {
  agent: AgentViewInput;
  viewId?: string;
}) {
  const [activeTab, setActiveTab] = useState<WorkspaceTab>(() => viewIdToTab(viewId));

  const initialSystemPrompt = isPersistedAgent(agent)
    ? (agent.systemPrompt ?? '')
    : `### Business Context\n${agent.name} is designed to solve one crisp job exceptionally well.`
  const initialModel = isPersistedAgent(agent) ? (agent.model ?? DEFAULT_MODEL) : agent.model
  const initialTemp = isPersistedAgent(agent) ? (agent.temperature ?? 0.4) : 0.4

  const [systemPrompt, setSystemPrompt] = useState(initialSystemPrompt)
  const [model, setModel] = useState(initialModel)
  const [temperature, setTemperature] = useState(initialTemp)
  const [saveStatus, setSaveStatus] = useState<SaveStatus>('idle')
  const [showModelDropdown, setShowModelDropdown] = useState(false)

  const handleSave = useCallback(async () => {
    if (!isPersistedAgent(agent)) return
    setSaveStatus('saving')
    try {
      const res = await fetch(`/api/agents/${agent.id}`, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ systemPrompt, model, temperature }),
      })
      if (!res.ok) throw new Error('Save failed')
      setSaveStatus('saved')
      setTimeout(() => setSaveStatus('idle'), 2000)
    } catch {
      setSaveStatus('error')
      setTimeout(() => setSaveStatus('idle'), 3000)
    }
  }, [agent, systemPrompt, model, temperature])

  return (
    <div className="flex h-full overflow-hidden text-[#23252f] bg-white">

      {/* LEFT: Configuration Rail */}
      <div className="flex w-[380px] shrink-0 flex-col overflow-hidden border-r border-[#E9EBF2] bg-white">
        
        {/* Agent Header & Tabs */}
        <div className="border-b border-[#E9EBF2] px-5 py-5">
          <h1 className="text-lg font-semibold tracking-[-0.01em] text-[#111827]">{agent.name}</h1>
          <p className="mt-1 text-[13px] text-[#6B7280]">Manage configuration and test your agent.</p>
          
          <div className="mt-5 flex items-center space-x-1 rounded-lg bg-[#F3F4F6] p-1">
            <button
              onClick={() => setActiveTab('playground')}
              className={`flex flex-1 items-center justify-center gap-2 rounded-md py-1.5 text-[12px] font-medium transition-all ${
                activeTab === 'playground' ? 'bg-white text-[#111827] shadow-sm' : 'text-[#6B7280] hover:text-[#374151]'
              }`}
            >
              <Settings className="size-3.5" />
              Settings
            </button>
            {isPersistedAgent(agent) && resolveAgentProfile(agent) === 'deployed_agent' ? (
              <button
                onClick={() => setActiveTab('inbox')}
                className={`flex flex-1 items-center justify-center gap-2 rounded-md py-1.5 text-[12px] font-medium transition-all ${
                  activeTab === 'inbox' ? 'bg-white text-[#111827] shadow-sm' : 'text-[#6B7280] hover:text-[#374151]'
                }`}
                title="Operator inbox — runs, HITL approvals (deployed agents)"
              >
                <Inbox className="size-3.5" />
                Inbox
              </button>
            ) : null}
            <button
              onClick={() => setActiveTab('knowledge')}
              className={`flex flex-1 items-center justify-center gap-2 rounded-md py-1.5 text-[12px] font-medium transition-all ${
                activeTab === 'knowledge' ? 'bg-white text-[#111827] shadow-sm' : 'text-[#6B7280] hover:text-[#374151]'
              }`}
            >
              <Database className="size-3.5" />
              Sources
            </button>
            <button
              onClick={() => setActiveTab('actions')}
              className={`flex flex-1 items-center justify-center gap-2 rounded-md py-1.5 text-[12px] font-medium transition-all ${
                activeTab === 'actions' ? 'bg-white text-[#111827] shadow-sm' : 'text-[#6B7280] hover:text-[#374151]'
              }`}
            >
              <Code className="size-3.5" />
              Tools
            </button>
            <button
              onClick={() => setActiveTab('analytics')}
              className={`flex flex-1 items-center justify-center gap-2 rounded-md py-1.5 text-[12px] font-medium transition-all ${
                activeTab === 'analytics' ? 'bg-white text-[#111827] shadow-sm' : 'text-[#6B7280] hover:text-[#374151]'
              }`}
            >
              <Activity className="size-3.5" />
              Analytics
            </button>
            <button
              onClick={() => setActiveTab('schedules')}
              className={`flex flex-1 items-center justify-center gap-2 rounded-md py-1.5 text-[12px] font-medium transition-all ${
                activeTab === 'schedules' ? 'bg-white text-[#111827] shadow-sm' : 'text-[#6B7280] hover:text-[#374151]'
              }`}
              title="Per-agent scheduled runs (cron)"
            >
              <Calendar className="size-3.5" />
              Schedules
            </button>
            <button
              onClick={() => setActiveTab('finetune')}
              className={`flex flex-1 items-center justify-center gap-2 rounded-md py-1.5 text-[12px] font-medium transition-all ${
                activeTab === 'finetune' ? 'bg-white text-[#111827] shadow-sm' : 'text-[#6B7280] hover:text-[#374151]'
              }`}
              title="Fine-tune the agent's base model (Wave 7)"
            >
              <GraduationCap className="size-3.5" />
              Fine-tune
            </button>
            <button
              onClick={() => setActiveTab('embed')}
              className={`flex flex-1 items-center justify-center gap-2 rounded-md py-1.5 text-[12px] font-medium transition-all ${
                activeTab === 'embed' ? 'bg-white text-[#111827] shadow-sm' : 'text-[#6B7280] hover:text-[#374151]'
              }`}
              title="Deploy as a public chat bubble on any site (Wave 9)"
            >
              <Share2 className="size-3.5" />
              Embed
            </button>
          </div>
        </div>

        {/* Scrollable Tab Content */}
        <div className="flex-1 overflow-y-auto">
          {activeTab === 'playground' && (
            <div className="flex flex-col space-y-6 p-5">
              {/* Instructions */}
              <div>
                <div className="flex items-center justify-between">
                  <label className="text-[13px] font-medium text-[#111827]">System Prompt</label>
                  <button className="text-[12px] text-blue-600 hover:text-blue-700">Optimize</button>
                </div>
                <p className="mt-1 text-[12px] text-[#6B7280]">Give the AI a role and explicitly define its boundaries.</p>
                <textarea
                  className="mt-3 w-full min-h-[140px] resize-y rounded-xl border border-[#E5E7EB] bg-[#F9FAFB] p-3 text-[13px] text-[#374151] outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500"
                  value={systemPrompt}
                  onChange={(e) => setSystemPrompt(e.target.value)}
                />
              </div>

              {/* Model Selection */}
              <div className="relative">
                <label className="text-[13px] font-medium text-[#111827]">AI Model</label>
                <button
                  type="button"
                  onClick={() => setShowModelDropdown((v) => !v)}
                  className="mt-2 flex w-full items-center justify-between rounded-xl border border-[#E5E7EB] bg-white px-3.5 py-2.5 shadow-sm transition-all hover:bg-[#F9FAFB]"
                >
                  <div className="flex items-center gap-2">
                    <div className="size-5 rounded-full bg-blue-100 flex items-center justify-center">
                      <Sparkles className="size-3 text-blue-600" />
                    </div>
                    <span className="text-[13px] font-medium text-[#111827]">
                      {SUPPORTED_MODELS[model as SupportedModel]?.label ?? model}
                    </span>
                  </div>
                  <ChevronDown className="size-4 text-[#9CA3AF]" />
                </button>
                {showModelDropdown && (
                  <div className="absolute left-0 right-0 top-full z-20 mt-1 rounded-xl border border-[#E5E7EB] bg-white shadow-lg overflow-hidden">
                    {Object.entries(SUPPORTED_MODELS).map(([id, meta]) => (
                      <button
                        key={id}
                        type="button"
                        onClick={() => { setModel(id); setShowModelDropdown(false); }}
                        className={`w-full flex items-center gap-3 px-4 py-2.5 text-left text-[13px] hover:bg-gray-50 transition-colors ${id === model ? 'bg-blue-50/60 text-blue-700' : 'text-[#374151]'}`}
                      >
                        <Sparkles className="size-3.5 shrink-0" />
                        <div>
                          <div className="font-medium">{meta.label}</div>
                          <div className="text-[11px] text-gray-400">{meta.provider}</div>
                        </div>
                      </button>
                    ))}
                  </div>
                )}
              </div>

              {/* Identity & Creativity */}
              <div>
                <label className="text-[13px] font-medium text-[#111827]">Temperature (Creativity)</label>
                <div className="mt-3 flex items-center gap-4">
                  <input
                    type="range"
                    min="0"
                    max="100"
                    value={Math.round(temperature * 100)}
                    onChange={(e) => setTemperature(Number(e.target.value) / 100)}
                    className="flex-1 accent-blue-600"
                  />
                  <span className="w-8 text-right text-[12px] font-medium text-[#6B7280]">{temperature.toFixed(1)}</span>
                </div>
                <div className="mt-1 flex justify-between text-[11px] text-[#9CA3AF]">
                  <span>Precise</span>
                  <span>Creative</span>
                </div>
              </div>
            </div>
          )}

          {activeTab === 'knowledge' && (
            <KnowledgeTab
              agent={isPersistedAgent(agent) ? agent : null}
              agentName={agent.name}
            />
          )}

          {activeTab === 'actions' && (
            <ToolsTab
              agent={isPersistedAgent(agent) ? agent : null}
              agentName={agent.name}
            />
          )}

          {activeTab === 'analytics' && (
            <AnalyticsTab agentId={isPersistedAgent(agent) ? agent.id : undefined} />
          )}

          {activeTab === 'schedules' && (
            <SchedulesTab
              agent={isPersistedAgent(agent) ? agent : null}
              agentName={agent.name}
            />
          )}

          {activeTab === 'finetune' && (
            <FinetuneTab
              agent={isPersistedAgent(agent) ? agent : null}
              agentName={agent.name}
              currentModel={model}
            />
          )}

          {activeTab === 'embed' && (
            <EmbedTab
              agent={isPersistedAgent(agent) ? agent : null}
              agentName={agent.name}
            />
          )}
        </div>

        {/* Footer Actions */}
        <div className="border-t border-[#E9EBF2] bg-[#F9FAFB] p-5">
          <button
            type="button"
            onClick={handleSave}
            disabled={saveStatus === 'saving' || !isPersistedAgent(agent)}
            className="w-full flex items-center justify-center gap-2 rounded-xl bg-[#111827] px-4 py-2.5 text-[13px] font-medium text-white shadow-sm hover:bg-gray-800 transition-colors disabled:opacity-60 disabled:cursor-not-allowed"
          >
            {saveStatus === 'saving' && <Loader2 className="size-3.5 animate-spin" />}
            {saveStatus === 'saved' && <Check className="size-3.5" />}
            {saveStatus === 'idle' && 'Save & Publish Agent'}
            {saveStatus === 'saving' && 'Saving...'}
            {saveStatus === 'saved' && 'Saved'}
            {saveStatus === 'error' && 'Save failed — retry'}
          </button>
        </div>
      </div>

      {/* RIGHT: operator inbox (deployed_agent) or the live playground. */}
      {activeTab === 'inbox' && isPersistedAgent(agent) ? (
        <div className="flex-1 overflow-hidden">
          <AgentInbox agent={agent} />
        </div>
      ) : (
        <AgentPlaygroundPane
          agent={isPersistedAgent(agent) ? agent : null}
          agentName={agent.name}
          greeting={isPersistedAgent(agent) ? agent.greeting : undefined}
        />
      )}
    </div>
  );
}

/**
 * Wave 9 §19: per-turn tool-loop trace panel. Renders a collapsible
 * pill under each assistant message showing which tools fired during
 * the turn — what was passed in, what came back, how many bytes the
 * tool result was.
 *
 * Trace data flows: gateway tool_loop → http_routes InvokeResponse →
 * velion chat-stream metadata → useAgentPlayground message.toolTrace.
 */
function ToolTracePanel({
  trace,
}: {
  trace: NonNullable<PlaygroundMessage['toolTrace']>;
}): ReactElement {
  const [expanded, setExpanded] = useState<boolean>(false);
  return (
    <div className="max-w-[85%]">
      <button
        type="button"
        onClick={() => setExpanded((v) => !v)}
        className="inline-flex items-center gap-1.5 rounded-full bg-blue-50 px-2 py-0.5 text-[10px] font-medium text-blue-700 hover:bg-blue-100"
        aria-expanded={expanded}
      >
        <Code className="size-2.5" />
        {trace.length} {trace.length === 1 ? 'tool used' : 'tools used'}
        <ChevronDown
          className={`size-2.5 transition-transform ${expanded ? 'rotate-180' : ''}`}
        />
      </button>
      {expanded && (
        <ol className="mt-1.5 space-y-1.5 rounded-lg border border-blue-100 bg-blue-50/40 p-2">
          {trace.map((entry, i) => (
            <li key={`${entry.round}-${entry.tool}-${i}`} className="text-[11px] text-[#374151]">
              <div className="flex items-center gap-1.5">
                <span className="font-mono text-[10px] text-[#9CA3AF]">
                  round {entry.round}
                </span>
                <span className="font-semibold text-[#111827]">{entry.tool}</span>
                <span className="text-[10px] text-[#6B7280]">
                  ({entry.result_bytes}B)
                </span>
              </div>
              {entry.args_preview && (
                <div className="mt-0.5 font-mono text-[10px] text-[#6B7280] truncate">
                  args: {entry.args_preview}
                </div>
              )}
              {entry.result_preview && (
                <div className="mt-0.5 font-mono text-[10px] text-[#6B7280] truncate">
                  result: {entry.result_preview}
                </div>
              )}
            </li>
          ))}
        </ol>
      )}
    </div>
  );
}

interface PlaygroundPaneProps {
  agent: PersistedAgent | null;
  agentName: string;
  greeting?: string;
}

function AgentPlaygroundPane({ agent, agentName, greeting }: PlaygroundPaneProps): ReactElement {
  const [input, setInput] = useState('');
  const { messages, isStreaming, send, reset, rateMessage, runEvents, profile } = useAgentPlayground({
    agent,
    greeting,
  });

  // Wave 11 §5: index of the most recent assistant turn — drives the
  // keyboard-shortcut binding on the rating chips (G/A/P).
  const latestAssistantIndex = (() => {
    for (let i = messages.length - 1; i >= 0; i--) {
      if (messages[i].role === 'assistant' && messages[i].runId) return i;
    }
    return -1;
  })();

  const canSend = Boolean(agent) && input.trim().length > 0 && !isStreaming;

  const handleSubmit = useCallback(
    async (e?: React.FormEvent) => {
      e?.preventDefault();
      if (!canSend) return;
      const text = input;
      setInput('');
      await send(text);
    },
    [canSend, input, send],
  );

  return (
    <div className="relative flex flex-1 flex-col items-center justify-center overflow-y-auto bg-[#F9FAFB] px-10 py-10">
      <div className="pointer-events-none absolute inset-0 bg-[#F9FAFB] [background-image:linear-gradient(to_right,#E5E7EB_1px,transparent_1px),linear-gradient(to_bottom,#E5E7EB_1px,transparent_1px)] [background-size:24px_24px] [mask-image:radial-gradient(ellipse_60%_60%_at_50%_50%,#000_10%,transparent_100%)] opacity-50" />

      <div className="z-10 mb-6 flex flex-col items-center text-center">
        <div className="inline-flex items-center gap-2 rounded-full border border-gray-200 bg-white px-3 py-1 shadow-sm">
          <span className={`flex size-2 items-center justify-center rounded-full ${agent ? 'bg-green-500' : 'bg-amber-400'}`}></span>
          <span className="text-[12px] font-medium text-gray-600">
            {agent ? 'Live Playground' : 'Save agent to enable playground'}
          </span>
        </div>
        <h2 className="mt-4 text-xl font-semibold text-gray-900">Test your agent</h2>
        <p className="mt-1 max-w-[320px] text-[13px] text-gray-500">
          Talk to {agentName} using its current configuration. Replies go
          through the Model Plane with the agent's system prompt, model,
          and tool list active.
        </p>
      </div>

      <div className="relative z-10 flex w-full max-w-[860px] items-start justify-center gap-4">
      <div className="flex w-full max-w-[400px] flex-col overflow-hidden rounded-2xl border border-[#E5E7EB] bg-white shadow-xl h-[600px] max-h-full">
        <div className="flex items-center justify-between border-b border-[#E5E7EB] bg-white px-5 py-4">
          <div className="flex items-center gap-3">
            <div className="flex size-8 items-center justify-center rounded-full bg-gradient-to-tr from-blue-600 to-violet-600">
              <Sparkles className="size-4 text-white" strokeWidth={2} />
            </div>
            <div>
              <h3 className="text-[14px] font-semibold text-[#111827]">{agentName}</h3>
              <p className="text-[11px] text-gray-500">
                {isStreaming ? 'Thinking…' : 'Replies via Model Plane'}
              </p>
            </div>
          </div>
          <button
            type="button"
            onClick={reset}
            className="rounded-full p-1.5 text-gray-400 hover:bg-gray-100 hover:text-gray-600 transition-colors"
            aria-label="Reset conversation"
          >
            <RefreshCcw className="size-4" />
          </button>
        </div>

        <div className="flex-1 overflow-y-auto bg-white p-5 space-y-4">
          {messages.map((m, idx) => (
            <div
              key={m.id}
              className={`flex flex-col gap-1 ${m.role === 'user' ? 'items-end' : 'items-start'}`}
            >
              <div
                className={
                  m.role === 'user'
                    ? 'max-w-[85%] rounded-2xl rounded-tr-sm bg-blue-600 px-4 py-2.5 text-[13px] leading-relaxed text-white'
                    : 'max-w-[85%] rounded-2xl rounded-tl-sm bg-gray-100 px-4 py-2.5 text-[13px] leading-relaxed text-gray-800'
                }
              >
                {m.streaming && !m.content ? (
                  <span className="flex items-center gap-1.5">
                    <span className="size-1.5 rounded-full bg-blue-600 animate-pulse" />
                    <span className="size-1.5 rounded-full bg-blue-600 animate-pulse [animation-delay:75ms]" />
                    <span className="size-1.5 rounded-full bg-blue-600 animate-pulse [animation-delay:150ms]" />
                  </span>
                ) : (
                  <span className="whitespace-pre-wrap">{m.content}</span>
                )}
              </div>
              {m.error && (
                <span className="text-[10px] text-rose-500">
                  {m.error}
                </span>
              )}
              {/* Wave 9 §19: per-turn tool-loop trace. Collapsed by
                  default — click the pill to expand each round's
                  args + result snippet. */}
              {m.role === 'assistant' && m.toolTrace && m.toolTrace.length > 0 && (
                <ToolTracePanel trace={m.toolTrace} />
              )}
              {/* Wave 11 §5: Fin G/A/P feedback chips. Only on assistant
                  turns that have a real runId (welcome msg + errored
                  turns are skipped). Keyboard shortcuts bind to the
                  most recent rated turn. */}
              {m.role === 'assistant' && !m.streaming && m.runId && !m.error && (
                <PlaygroundRatingChips
                  message={m}
                  onRate={(rating) => rateMessage(m.id, rating)}
                  isLatest={idx === latestAssistantIndex}
                />
              )}
            </div>
          ))}
        </div>

        <form onSubmit={handleSubmit} className="border-t border-[#E5E7EB] bg-white p-3">
          <div className="flex items-center gap-2 rounded-xl bg-gray-50 px-3 py-2 border border-gray-200 focus-within:border-blue-500 focus-within:ring-1 focus-within:ring-blue-500 transition-all">
            <input
              type="text"
              value={input}
              onChange={(e) => setInput(e.target.value)}
              placeholder={agent ? 'Ask me anything…' : 'Save the agent first…'}
              disabled={!agent || isStreaming}
              className="flex-1 bg-transparent text-[13px] text-gray-800 outline-none placeholder:text-gray-400 disabled:opacity-50"
            />
            <button
              type="submit"
              disabled={!canSend}
              className="grid size-8 place-items-center rounded-lg bg-blue-600 text-white shadow-sm transition-colors hover:bg-blue-700 disabled:opacity-40 disabled:cursor-not-allowed"
              aria-label="Send"
            >
              <Send className="size-3.5 ml-0.5" />
            </button>
          </div>
          <div className="mt-2 text-center">
            <span className="text-[10px] font-medium text-gray-400">
              Powered by Velion Model Plane
            </span>
          </div>
        </form>
      </div>

      {/* Operator surface — only for deployed agents (Intercom/Zendesk-style).
          Chat-profile agents keep the clean single-pane experience. */}
      {profile === 'deployed_agent' ? <RunEventPanel events={runEvents} /> : null}
      </div>
    </div>
  );
}

/**
 * Operator-facing run-event panel (deployed_agent profile). Renders the shared
 * task-graph feed from `useRunEvents` — connection status, HITL pause, and the
 * latest plan/todo/approval transitions for the most recent test run.
 */
function RunEventPanel({ events }: { events: RunEventsState }): ReactElement {
  const plans = Object.values(events.plans);
  const todos = Object.values(events.todos);
  const approvals = Object.values(events.approvals);
  const statusLabel =
    events.status === 'open'
      ? 'Live'
      : events.status === 'connecting'
        ? 'Connecting…'
        : events.status === 'closed'
          ? 'Closed'
          : 'Idle';

  return (
    <div className="flex w-full max-w-[300px] flex-col overflow-hidden rounded-2xl border border-[#E5E7EB] bg-white shadow-xl h-[600px] max-h-full">
      <div className="flex items-center justify-between border-b border-[#E5E7EB] px-4 py-3">
        <h3 className="text-[13px] font-semibold text-[#111827]">Run activity</h3>
        <span
          className={`inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-[10px] font-medium ${
            events.status === 'open'
              ? 'bg-green-50 text-green-700'
              : 'bg-gray-100 text-gray-500'
          }`}
        >
          <span
            className={`size-1.5 rounded-full ${
              events.status === 'open' ? 'bg-green-500' : 'bg-gray-400'
            }`}
          />
          {statusLabel}
        </span>
      </div>

      <div className="flex-1 space-y-4 overflow-y-auto p-4 text-[12px]">
        {events.paused && (
          <div className="rounded-lg border border-amber-200 bg-amber-50 px-3 py-2 text-[11px] text-amber-800">
            Paused — awaiting approval
            {events.pendingApprovalId ? (
              <span className="block font-mono text-[10px] text-amber-600">
                {events.pendingApprovalId}
              </span>
            ) : null}
          </div>
        )}

        <RunEventSection title="Plan" empty={plans.length === 0}>
          {plans.map((p) => (
            <li key={p.planId} className="flex items-center justify-between gap-2">
              <span className="truncate font-mono text-[10px] text-gray-400">{p.planId}</span>
              <span className="text-gray-700">{p.to}</span>
            </li>
          ))}
        </RunEventSection>

        <RunEventSection title="Todos" empty={todos.length === 0}>
          {todos.map((t) => (
            <li key={t.todoId} className="flex items-center justify-between gap-2">
              <span className="truncate font-mono text-[10px] text-gray-400">{t.todoId}</span>
              <span className="text-gray-700">{t.to}</span>
            </li>
          ))}
        </RunEventSection>

        <RunEventSection title="Approvals" empty={approvals.length === 0}>
          {approvals.map((a) => (
            <li key={a.approvalId} className="flex items-center justify-between gap-2">
              <span className="truncate text-gray-700">{a.kind}</span>
              <span className="text-gray-500">{a.state}</span>
            </li>
          ))}
        </RunEventSection>
      </div>

      <div className="border-t border-[#E5E7EB] px-4 py-2 text-center text-[10px] text-gray-400">
        {events.lastEventId ? `cursor ${events.lastEventId}` : 'no events yet'}
      </div>
    </div>
  );
}

function RunEventSection({
  title,
  empty,
  children,
}: {
  title: string;
  empty: boolean;
  children: React.ReactNode;
}): ReactElement {
  return (
    <div>
      <h4 className="mb-1 text-[10px] font-semibold uppercase tracking-wider text-gray-400">
        {title}
      </h4>
      {empty ? (
        <p className="text-[11px] text-gray-400">—</p>
      ) : (
        <ul className="space-y-1">{children}</ul>
      )}
    </div>
  );
}

/**
 * U3-9 (ui-ux-velion-gap.md §14): real analytics tab.
 *
 * Reads from `/api/agents/{agentId}/stats` which aggregates the Convex
 * `agentRuns` mirror (populated by `convex-subscriber` from
 * orchestrator-core's NATS events per U3-3 / W4-2). Replaces the
 * previously hardcoded "1,248 conversations / 76.2% deflection" cards.
 *
 * When the agent has no runs yet (new agent, or stats unavailable), the
 * panel renders an honest "No runs yet" empty state rather than fake
 * numbers — a "fallback %" of 0 with zero runs is meaningless and shows
 * better than a misleading non-zero placeholder.
 */
function AnalyticsTab({ agentId }: { agentId: string | undefined }): ReactElement {
  const { data: stats, isLoading } = useAgentStats(agentId);

  if (!agentId) {
    return (
      <div className="flex flex-col space-y-6 p-5">
        <div className="rounded-xl border border-[#E5E7EB] bg-[#F9FAFB] p-5 text-center">
          <Activity className="mx-auto size-5 text-[#9CA3AF]" />
          <p className="mt-2 text-[13px] font-medium text-[#374151]">
            Save the agent first
          </p>
          <p className="mt-1 text-[12px] text-[#6B7280]">
            Analytics appears once the agent is persisted and has run at
            least once.
          </p>
        </div>
      </div>
    );
  }

  if (isLoading && stats.total === 0) {
    return (
      <div className="flex flex-col space-y-3 p-5">
        <div className="h-20 animate-pulse rounded-xl bg-[#F3F4F6]" />
        <div className="h-20 animate-pulse rounded-xl bg-[#F3F4F6]" />
      </div>
    );
  }

  const formatDuration = (ms: number): string => {
    if (!ms || ms < 1000) return '—';
    const totalSec = Math.round(ms / 1000);
    const minutes = Math.floor(totalSec / 60);
    const seconds = totalSec % 60;
    return minutes > 0 ? `${minutes}m ${seconds}s` : `${seconds}s`;
  };

  const formatRelative = (ms: number): string => {
    const diff = Date.now() - ms;
    if (diff < 60_000) return 'just now';
    if (diff < 3_600_000) return `${Math.round(diff / 60_000)}m ago`;
    if (diff < 86_400_000) return `${Math.round(diff / 3_600_000)}h ago`;
    return `${Math.round(diff / 86_400_000)}d ago`;
  };

  return (
    <div className="flex flex-col space-y-6 p-5">
      <div>
        <div className="flex items-center justify-between">
          <label className="text-[13px] font-medium text-[#111827]">
            Performance Overview
          </label>
          <span className="text-[11px] text-[#9CA3AF]">
            last {stats.lookbackDays} days
          </span>
        </div>

        {stats.total === 0 ? (
          <div className="mt-3 rounded-xl border border-dashed border-[#D1D5DB] bg-[#F9FAFB] p-6 text-center">
            <p className="text-[13px] font-medium text-[#374151]">
              No runs yet
            </p>
            <p className="mt-1 text-[12px] text-[#6B7280]">
              Trigger the agent from the playground or via /v1/invoke to
              start collecting metrics.
            </p>
          </div>
        ) : (
          <div className="mt-4 grid grid-cols-2 gap-3">
            <div className="rounded-xl border border-[#E5E7EB] bg-white p-4">
              <p className="text-[12px] text-[#6B7280]">Total runs</p>
              <p className="mt-1 text-2xl font-semibold text-[#111827]">
                {stats.total}
              </p>
            </div>
            <div className="rounded-xl border border-[#E5E7EB] bg-white p-4">
              <p className="text-[12px] text-[#6B7280]">Success rate</p>
              <p className="mt-1 text-2xl font-semibold text-[#111827]">
                {Math.round(stats.successRate * 100)}%
              </p>
            </div>
            <div className="rounded-xl border border-[#E5E7EB] bg-white p-4">
              <p className="text-[12px] text-[#6B7280]">Avg duration</p>
              <p className="mt-1 text-2xl font-semibold text-[#111827]">
                {formatDuration(stats.avgDurationMs)}
              </p>
            </div>
            <div className="rounded-xl border border-[#E5E7EB] bg-white p-4">
              <p className="text-[12px] text-[#6B7280]">Failed</p>
              <p className="mt-1 text-2xl font-semibold text-[#111827]">
                {stats.failed}
              </p>
            </div>
          </div>
        )}
      </div>

      {stats.recent.length > 0 && (
        <div>
          <label className="text-[13px] font-medium text-[#111827]">
            Recent runs
          </label>
          <ul className="mt-3 space-y-1.5">
            {stats.recent.map((run) => (
              <li
                key={run.runId}
                className="flex items-center justify-between rounded-lg border border-[#E5E7EB] bg-white px-3 py-2 text-[12px]"
              >
                <span className="flex items-center gap-2">
                  <span
                    className={`inline-block size-1.5 rounded-full ${
                      run.status === 'completed'
                        ? 'bg-emerald-500'
                        : run.status === 'failed'
                          ? 'bg-rose-500'
                          : run.status === 'started'
                            ? 'bg-amber-400 animate-pulse'
                            : 'bg-zinc-300'
                    }`}
                  />
                  <span className="font-mono text-[11px] text-[#6B7280]">
                    {run.runId.slice(0, 12)}…
                  </span>
                </span>
                <span className="flex items-center gap-3 text-[11px] text-[#9CA3AF]">
                  <span>{run.status}</span>
                  <span>{formatRelative(run.startedAt)}</span>
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

/**
 * U3-8 (ui-ux-velion-gap.md §14): Tools tab.
 *
 * Renders the merged catalog of:
 *  - Built-in Model Plane tools (Browse Web, Deep Research, Fetch URL,
 *    Image generation) wired directly to the gateway HTTP routes.
 *  - Custom skills from capability-core via `/api/skills`.
 *
 * Toggling persists via `PATCH /api/agents/{id}` with the new `tools`
 * array; the gateway reads `tools` off the agent record on each invoke
 * and decides which tool-use loop steps to run.
 */
function ToolsTab({
  agent,
  agentName,
}: {
  agent: PersistedAgent | null;
  agentName: string;
}): ReactElement {
  const { tools, toggle, save, dirty, saveStatus, isLoading, unavailable } =
    useAgentTools(agent);

  if (!agent) {
    return (
      <div className="flex flex-col space-y-4 p-5">
        <div className="rounded-xl border border-[#E5E7EB] bg-[#F9FAFB] p-5 text-center">
          <Code className="mx-auto size-5 text-[#9CA3AF]" />
          <p className="mt-2 text-[13px] font-medium text-[#374151]">
            Save the agent first
          </p>
          <p className="mt-1 text-[12px] text-[#6B7280]">
            Tools are stored on the agent record. Save once, then come back
            here to flip capabilities on/off.
          </p>
        </div>
      </div>
    );
  }

  const builtins = tools.filter((t) => t.kind === 'builtin');
  const skills = tools.filter((t) => t.kind === 'skill');

  return (
    <div className="flex flex-col space-y-6 p-5">
      <div>
        <div className="flex items-center justify-between">
          <label className="text-[13px] font-medium text-[#111827]">Tools</label>
          {dirty && (
            <button
              type="button"
              onClick={save}
              disabled={saveStatus === 'saving'}
              className="inline-flex items-center gap-1.5 rounded-md bg-[#111827] px-2.5 py-1 text-[11px] font-medium text-white hover:bg-gray-800 disabled:opacity-60"
            >
              {saveStatus === 'saving' && <Loader2 className="size-3 animate-spin" />}
              {saveStatus === 'saved' && <Check className="size-3" />}
              {saveStatus === 'idle' && 'Save tools'}
              {saveStatus === 'saving' && 'Saving…'}
              {saveStatus === 'saved' && 'Saved'}
              {saveStatus === 'error' && 'Retry save'}
            </button>
          )}
        </div>
        <p className="mt-1 text-[12px] text-[#6B7280]">
          Enable the capabilities {agentName} can call during a conversation.
          Built-in tools are wired to the Model Plane gateway; skills come
          from your org's capability registry.
        </p>
      </div>

      <section>
        <p className="text-[11px] font-semibold uppercase tracking-wide text-[#9CA3AF]">
          Built-in
        </p>
        <ul className="mt-2 space-y-2">
          {builtins.map((tool) => (
            <li
              key={tool.id}
              className={`rounded-xl border p-3 transition-colors ${
                tool.enabled
                  ? 'border-blue-300 bg-blue-50/40'
                  : 'border-[#E5E7EB] bg-white hover:bg-gray-50'
              }`}
            >
              <label className="flex cursor-pointer items-start justify-between gap-3">
                <span>
                  <span className="block text-[13px] font-medium text-[#111827]">
                    {tool.name}
                  </span>
                  <span className="mt-0.5 block text-[12px] text-[#6B7280]">
                    {tool.description}
                  </span>
                </span>
                <input
                  type="checkbox"
                  checked={tool.enabled}
                  onChange={() => toggle(tool.id)}
                  className="mt-1 size-4 accent-blue-600"
                />
              </label>
            </li>
          ))}
        </ul>
      </section>

      <section>
        <div className="flex items-center justify-between">
          <p className="text-[11px] font-semibold uppercase tracking-wide text-[#9CA3AF]">
            Org skills
          </p>
          <span className="text-[11px] text-[#9CA3AF]">
            from capability-core
          </span>
        </div>
        {isLoading ? (
          <div className="mt-2 h-12 animate-pulse rounded-xl bg-[#F3F4F6]" />
        ) : unavailable ? (
          <div className="mt-2 rounded-xl border border-dashed border-[#D1D5DB] bg-[#F9FAFB] p-4 text-center text-[12px] text-[#6B7280]">
            Skills registry unavailable — capability-core isn't reachable.
            Built-in tools still work.
          </div>
        ) : skills.length === 0 ? (
          <div className="mt-2 rounded-xl border border-dashed border-[#D1D5DB] bg-[#F9FAFB] p-4 text-center text-[12px] text-[#6B7280]">
            No custom skills yet — create one in the capability registry to
            see it here.
          </div>
        ) : (
          <ul className="mt-2 space-y-2">
            {skills.map((tool) => (
              <li
                key={tool.id}
                className={`rounded-xl border p-3 transition-colors ${
                  tool.enabled
                    ? 'border-violet-300 bg-violet-50/40'
                    : 'border-[#E5E7EB] bg-white hover:bg-gray-50'
                }`}
              >
                <label className="flex cursor-pointer items-start justify-between gap-3">
                  <span>
                    <span className="block text-[13px] font-medium text-[#111827]">
                      {tool.name}
                    </span>
                    <span className="mt-0.5 block text-[12px] text-[#6B7280]">
                      {tool.description}
                    </span>
                  </span>
                  <input
                    type="checkbox"
                    checked={tool.enabled}
                    onChange={() => toggle(tool.id)}
                    className="mt-1 size-4 accent-violet-600"
                  />
                </label>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}

/**
 * U3-7 (ui-ux-velion-gap.md §14): Knowledge tab.
 *
 * Manages the agent's `knowledgeSources` array. File uploads stream through
 * the existing `/api/chat/upload` proxy (Data Plane documents-api) and the
 * returned document id is appended to the agent record. URL sources are
 * stored as references and resolved at invoke-time by the gateway's web
 * tools — no eager crawl from this UI.
 */
function KnowledgeTab({
  agent,
  agentName,
}: {
  agent: PersistedAgent | null;
  agentName: string;
}): ReactElement {
  const { sources, uploadFile, addUrl, remove, status, error } =
    useAgentKnowledge(agent);
  const [urlDraft, setUrlDraft] = useState('');

  if (!agent) {
    return (
      <div className="flex flex-col space-y-4 p-5">
        <div className="rounded-xl border border-[#E5E7EB] bg-[#F9FAFB] p-5 text-center">
          <Database className="mx-auto size-5 text-[#9CA3AF]" />
          <p className="mt-2 text-[13px] font-medium text-[#374151]">
            Save the agent first
          </p>
          <p className="mt-1 text-[12px] text-[#6B7280]">
            Knowledge sources are stored on the agent record. Save once,
            then come back here to add files or URLs.
          </p>
        </div>
      </div>
    );
  }

  const isWorking = status === 'saving';

  return (
    <div className="flex flex-col space-y-6 p-5">
      <div>
        <label className="text-[13px] font-medium text-[#111827]">
          Knowledge sources
        </label>
        <p className="mt-1 text-[12px] text-[#6B7280]">
          Files and URLs {agentName} can reference during a conversation.
          Files go through the Data Plane indexer; URLs are fetched on
          demand by the agent's browse tool.
        </p>
      </div>

      <label
        className={`flex cursor-pointer flex-col items-center justify-center rounded-xl border border-dashed p-6 text-center transition-colors ${
          isWorking
            ? 'border-blue-300 bg-blue-50/40'
            : 'border-[#D1D5DB] bg-[#F9FAFB] hover:bg-gray-50'
        }`}
      >
        <input
          type="file"
          className="sr-only"
          disabled={isWorking}
          onChange={async (e) => {
            const file = e.target.files?.[0];
            e.target.value = '';
            if (file) await uploadFile(file);
          }}
        />
        <Database className="mx-auto size-6 text-[#9CA3AF]" />
        <p className="mt-2 text-[13px] font-medium text-[#374151]">
          {isWorking ? 'Uploading…' : 'Upload a file'}
        </p>
        <p className="mt-1 text-[12px] text-[#6B7280]">
          PDF, DOCX, MD, TXT — sent to documents-api for indexing
        </p>
      </label>

      <div>
        <p className="text-[12px] font-medium text-[#374151]">Add a URL</p>
        <form
          className="mt-2 flex items-center gap-2"
          onSubmit={async (e) => {
            e.preventDefault();
            if (!urlDraft.trim()) return;
            await addUrl(urlDraft);
            setUrlDraft('');
          }}
        >
          <input
            type="url"
            value={urlDraft}
            onChange={(e) => setUrlDraft(e.target.value)}
            placeholder="https://docs.example.com/guide"
            className="flex-1 rounded-lg border border-[#E5E7EB] bg-white px-3 py-1.5 text-[12px] outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500"
          />
          <button
            type="submit"
            disabled={!urlDraft.trim() || isWorking}
            className="rounded-lg bg-[#111827] px-3 py-1.5 text-[12px] font-medium text-white hover:bg-gray-800 disabled:opacity-50"
          >
            Add
          </button>
        </form>
      </div>

      {error && (
        <p className="rounded-lg bg-rose-50 px-3 py-2 text-[11px] text-rose-700">
          {error}
        </p>
      )}

      {sources.length > 0 ? (
        <ul className="space-y-1.5">
          {sources.map((src, idx) => (
            <li
              key={`${src.type}-${src.name}-${idx}`}
              className="flex items-center justify-between rounded-lg border border-[#E5E7EB] bg-white px-3 py-2"
            >
              <span className="flex items-center gap-2 min-w-0 flex-1">
                <Database className="size-3.5 shrink-0 text-[#9CA3AF]" />
                <span className="truncate text-[12px] text-[#374151]">
                  {src.name}
                </span>
                <span className="shrink-0 rounded-full bg-[#F3F4F6] px-1.5 py-0.5 text-[10px] uppercase tracking-wide text-[#6B7280]">
                  {src.type}
                </span>
              </span>
              <button
                type="button"
                onClick={() => remove(idx)}
                aria-label={`Remove ${src.name}`}
                className="ml-3 rounded-md p-1 text-[#9CA3AF] hover:bg-gray-100 hover:text-rose-600"
              >
                <X className="size-3.5" />
              </button>
            </li>
          ))}
        </ul>
      ) : (
        <p className="text-[12px] text-[#9CA3AF]">
          No sources yet. Upload a file or paste a URL above.
        </p>
      )}
    </div>
  );
}

/**
 * U3-12 (ui-ux-velion-gap.md §14): per-agent cron schedules.
 *
 * Lists existing schedules for the agent, lets the operator add a
 * standard 5-field cron expression + a prompt that gets sent into the
 * agent on each tick. Backed by capability-core's cron table via the
 * gateway's `/v1/cron` surface.
 *
 * The form deliberately accepts the raw 5-field cron format (e.g.
 * `0 9 * * 1-5`) rather than a friendlier picker — that picker is a
 * separate UX task. The schedule field is required; everything else
 * has sensible defaults.
 */
function SchedulesTab({
  agent,
  agentName,
}: {
  agent: PersistedAgent | null;
  agentName: string;
}): ReactElement {
  const { entries, isLoading, error, create, remove, toggle } = useAgentCron(agent);
  const [draft, setDraft] = useState({ name: '', schedule: '', prompt: '' });
  const [submitting, setSubmitting] = useState(false);

  if (!agent) {
    return (
      <div className="flex flex-col space-y-4 p-5">
        <div className="rounded-xl border border-[#E5E7EB] bg-[#F9FAFB] p-5 text-center">
          <Calendar className="mx-auto size-5 text-[#9CA3AF]" />
          <p className="mt-2 text-[13px] font-medium text-[#374151]">
            Save the agent first
          </p>
          <p className="mt-1 text-[12px] text-[#6B7280]">
            Schedules attach to a saved agent id. Save once, then come
            back to set up recurring runs.
          </p>
        </div>
      </div>
    );
  }

  const canSubmit =
    draft.schedule.trim().length > 0 && draft.prompt.trim().length > 0 && !submitting;

  return (
    <div className="flex flex-col space-y-6 p-5">
      <div>
        <label className="text-[13px] font-medium text-[#111827]">Schedules</label>
        <p className="mt-1 text-[12px] text-[#6B7280]">
          Recurring jobs that run {agentName} on a cron schedule. Each tick
          sends the prompt below to the Model Plane with the agent's full
          config (system prompt, tools, knowledge sources).
        </p>
      </div>

      <form
        className="space-y-3 rounded-xl border border-[#E5E7EB] bg-white p-3"
        onSubmit={async (e) => {
          e.preventDefault();
          if (!canSubmit) return;
          setSubmitting(true);
          await create({
            name: draft.name,
            schedule: draft.schedule,
            prompt: draft.prompt,
          });
          setDraft({ name: '', schedule: '', prompt: '' });
          setSubmitting(false);
        }}
      >
        <div className="grid grid-cols-2 gap-2">
          <input
            value={draft.name}
            onChange={(e) => setDraft({ ...draft, name: e.target.value })}
            placeholder="Name (e.g. Daily digest)"
            className="rounded-lg border border-[#E5E7EB] bg-white px-3 py-1.5 text-[12px] outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500"
          />
          <input
            value={draft.schedule}
            onChange={(e) => setDraft({ ...draft, schedule: e.target.value })}
            placeholder="Cron (e.g. 0 9 * * 1-5)"
            className="rounded-lg border border-[#E5E7EB] bg-white px-3 py-1.5 text-[12px] font-mono outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500"
            required
          />
        </div>
        <textarea
          value={draft.prompt}
          onChange={(e) => setDraft({ ...draft, prompt: e.target.value })}
          placeholder="Prompt to send on every tick"
          rows={3}
          className="w-full resize-y rounded-lg border border-[#E5E7EB] bg-white px-3 py-1.5 text-[12px] outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500"
          required
        />
        <button
          type="submit"
          disabled={!canSubmit}
          className="w-full rounded-lg bg-[#111827] px-3 py-1.5 text-[12px] font-medium text-white hover:bg-gray-800 disabled:opacity-50"
        >
          {submitting ? 'Adding…' : 'Add schedule'}
        </button>
      </form>

      {error && (
        <p className="rounded-lg bg-rose-50 px-3 py-2 text-[11px] text-rose-700">
          {error}
        </p>
      )}

      <div>
        {isLoading ? (
          <div className="h-16 animate-pulse rounded-xl bg-[#F3F4F6]" />
        ) : entries.length === 0 ? (
          <p className="text-[12px] text-[#9CA3AF]">
            No schedules yet. Add one above and it'll trigger automatically.
          </p>
        ) : (
          <ul className="space-y-1.5">
            {entries.map((entry: CronEntry) => (
              <li
                key={entry.id}
                className="flex items-center justify-between gap-3 rounded-lg border border-[#E5E7EB] bg-white px-3 py-2"
              >
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[13px] font-medium text-[#111827]">
                    {entry.name || 'Untitled'}
                  </span>
                  <span className="block truncate font-mono text-[11px] text-[#6B7280]">
                    {entry.schedule}
                  </span>
                </span>
                <label className="flex shrink-0 items-center gap-1.5 text-[11px] text-[#6B7280]">
                  <input
                    type="checkbox"
                    checked={entry.enabled}
                    onChange={(e) => toggle(entry.id, e.target.checked)}
                    className="size-3.5 accent-blue-600"
                  />
                  {entry.enabled ? 'on' : 'off'}
                </label>
                <button
                  type="button"
                  onClick={() => remove(entry.id)}
                  aria-label={`Remove ${entry.name}`}
                  className="rounded-md p-1 text-[#9CA3AF] hover:bg-gray-100 hover:text-rose-600"
                >
                  <X className="size-3.5" />
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}

/**
 * Wave 7 (velion ui-ux-velion-gap.md §17): Fine-tune tab.
 *
 * Lets an org owner / admin upload a JSONL training dataset, kick off a
 * fine-tune job against the agent's base model, and (when the job
 * succeeds) explicitly promote the resulting fine-tuned deployment to
 * the agent's `model` field.
 *
 * Architecture notes:
 *   - JSONL validation runs client-side (per-line parse + key check)
 *     before the upload so operators see "X examples ready / Y rejected"
 *     up front, not after a 5-minute Azure round-trip.
 *   - Only owner/admin role users can see the kickoff form — non-admins
 *     still see the job list (read-only) so they can monitor in-flight
 *     work without being able to start more.
 *   - Auto-promotion is intentionally NOT implemented; a "Publish to
 *     agent" button on each succeeded job PATCHes `agent.model`
 *     explicitly. See §17 of the gap doc for the rationale (running
 *     experiments without affecting production traffic).
 */
function FinetuneTab({
  agent,
  agentName,
  currentModel,
}: {
  agent: PersistedAgent | null;
  agentName: string;
  currentModel: string;
}): ReactElement {
  const { jobs, isLoading, error, createJob, cancelJob } = useAgentFinetune(agent);
  const [file, setFile] = useState<File | null>(null);
  const [baseModel, setBaseModel] = useState<string>(currentModel || 'gpt-4o-mini');
  const [submitting, setSubmitting] = useState<boolean>(false);
  const [parseResult, setParseResult] = useState<{
    valid: number;
    invalid: number;
  } | null>(null);
  const [publishingId, setPublishingId] = useState<string | null>(null);

  const onFilePicked = async (f: File | null): Promise<void> => {
    setFile(f);
    setParseResult(null);
    if (!f) return;
    // Client-side JSONL validation. We accept either chat-format
    // (`messages`) or completion-format (`prompt`+`completion`); reject
    // anything else.
    const text = await f.text();
    let valid = 0;
    let invalid = 0;
    for (const raw of text.split(/\r?\n/)) {
      const line = raw.trim();
      if (!line) continue;
      try {
        const obj = JSON.parse(line) as Record<string, unknown>;
        if (
          (Array.isArray(obj.messages) && obj.messages.length > 0) ||
          (typeof obj.prompt === 'string' && typeof obj.completion === 'string')
        ) {
          valid += 1;
        } else {
          invalid += 1;
        }
      } catch {
        invalid += 1;
      }
    }
    setParseResult({ valid, invalid });
  };

  const handleSubmit = useCallback(async (): Promise<void> => {
    if (!agent || !file) return;
    setSubmitting(true);
    try {
      await createJob({ file, baseModel });
      setFile(null);
      setParseResult(null);
    } finally {
      setSubmitting(false);
    }
  }, [agent, file, baseModel, createJob]);

  const handlePublish = useCallback(
    async (job: FinetuneJob): Promise<void> => {
      if (!agent || !job.fine_tuned_model) return;
      setPublishingId(job.job_id);
      try {
        // Prefer the Azure deployment_name when present — that's the
        // routable handle the gateway uses to dispatch traffic. Fall
        // back to fine_tuned_model only if the poller hasn't yet
        // provisioned the deployment (transient state on the
        // running→succeeded boundary).
        const res = await fetch(`/api/agents/${agent.id}`, {
          method: 'PATCH',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            model: job.deployment_name || job.fine_tuned_model,
          }),
        });
        if (!res.ok) {
          throw new Error(`publish failed (${res.status})`);
        }
        // Optional: refresh the agent record locally. For now the next
        // page render reads from Convex and sees the updated model.
      } finally {
        setPublishingId(null);
      }
    },
    [agent],
  );

  if (!agent) {
    return (
      <div className="flex flex-col space-y-4 p-5">
        <div className="rounded-xl border border-[#E5E7EB] bg-[#F9FAFB] p-5 text-center">
          <GraduationCap className="mx-auto size-5 text-[#9CA3AF]" />
          <p className="mt-2 text-[13px] font-medium text-[#374151]">
            Save the agent first
          </p>
          <p className="mt-1 text-[12px] text-[#6B7280]">
            Fine-tune jobs attach to a saved agent id. Save once, then
            come back to kick off training.
          </p>
        </div>
      </div>
    );
  }

  const canSubmit =
    Boolean(file) && (parseResult?.valid ?? 0) > 0 && !submitting;

  return (
    <div className="flex flex-col space-y-6 p-5">
      <div>
        <label className="text-[13px] font-medium text-[#111827]">
          Fine-tune {agentName}
        </label>
        <p className="mt-1 text-[12px] text-[#6B7280]">
          Upload a JSONL dataset (chat-format <code>{`{"messages":[...]}`}</code> or
          completion-format <code>{`{"prompt":"...","completion":"..."}`}</code> per
          line). Azure runs the training; on success, "Publish to agent"
          swaps the agent's model to the fine-tuned deployment.
        </p>
        <p className="mt-1 text-[11px] text-[#9CA3AF]">
          Current model: <code className="font-mono">{currentModel}</code> ·
          Fine-tuning is Azure-only · org admin role required to kick off jobs.
        </p>
      </div>

      <form
        className="space-y-3 rounded-xl border border-[#E5E7EB] bg-white p-3"
        onSubmit={(e) => {
          e.preventDefault();
          void handleSubmit();
        }}
      >
        <label className="flex cursor-pointer flex-col items-center justify-center rounded-lg border border-dashed border-[#D1D5DB] bg-[#F9FAFB] p-4 text-center hover:bg-gray-50">
          <input
            type="file"
            accept=".jsonl,application/jsonl,application/json"
            className="sr-only"
            disabled={submitting}
            onChange={(e) => {
              void onFilePicked(e.target.files?.[0] ?? null);
            }}
          />
          <GraduationCap className="size-5 text-[#9CA3AF]" />
          <p className="mt-2 text-[13px] font-medium text-[#374151]">
            {file ? file.name : 'Choose a JSONL dataset'}
          </p>
          {parseResult && (
            <p className="mt-1 text-[11px] text-[#6B7280]">
              {parseResult.valid} examples ready ·{' '}
              {parseResult.invalid > 0
                ? `${parseResult.invalid} rejected`
                : 'all valid'}
            </p>
          )}
        </label>

        <select
          value={baseModel}
          onChange={(e) => setBaseModel(e.target.value)}
          className="w-full rounded-lg border border-[#E5E7EB] bg-white px-3 py-1.5 text-[12px] outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500"
        >
          {/* Fine-tunable Azure deployments. Capability-core's models
              registry can later return a filtered list via
              ?finetunable=true; for now this short hardcoded list
              covers what the gateway's Azure deployment supports. */}
          <option value="gpt-4o-mini">gpt-4o-mini (recommended)</option>
          <option value="gpt-4o">gpt-4o</option>
        </select>

        <button
          type="submit"
          disabled={!canSubmit}
          className="w-full rounded-lg bg-[#111827] px-3 py-1.5 text-[12px] font-medium text-white hover:bg-gray-800 disabled:opacity-50"
        >
          {submitting ? 'Uploading + kicking off…' : 'Start fine-tune'}
        </button>
      </form>

      {error && (
        <p className="rounded-lg bg-rose-50 px-3 py-2 text-[11px] text-rose-700">
          {error}
        </p>
      )}

      <div>
        <label className="text-[13px] font-medium text-[#111827]">
          Past jobs
        </label>
        {isLoading ? (
          <div className="mt-2 h-16 animate-pulse rounded-xl bg-[#F3F4F6]" />
        ) : jobs.length === 0 ? (
          <p className="mt-2 text-[12px] text-[#9CA3AF]">
            No fine-tune jobs yet. Upload a dataset above to start.
          </p>
        ) : (
          <ul className="mt-2 space-y-1.5">
            {jobs.map((job) => (
              <li
                key={job.job_id}
                className="rounded-lg border border-[#E5E7EB] bg-white px-3 py-2"
              >
                <div className="flex items-center justify-between gap-3">
                  <span className="flex items-center gap-2 min-w-0 flex-1">
                    <span
                      className={`inline-block size-1.5 rounded-full ${
                        job.status === 'succeeded'
                          ? 'bg-emerald-500'
                          : job.status === 'failed'
                            ? 'bg-rose-500'
                            : job.status === 'running'
                              ? 'bg-amber-400 animate-pulse'
                              : job.status === 'cancelled'
                                ? 'bg-zinc-400'
                                : 'bg-blue-400'
                      }`}
                    />
                    <span className="truncate text-[12px] font-medium text-[#111827]">
                      {job.base_model}
                    </span>
                    <span className="shrink-0 rounded-full bg-[#F3F4F6] px-1.5 py-0.5 text-[10px] uppercase tracking-wide text-[#6B7280]">
                      {job.status}
                    </span>
                  </span>
                  <div className="flex items-center gap-2 shrink-0">
                    {job.status === 'succeeded' && job.fine_tuned_model && (
                      <button
                        type="button"
                        onClick={() => void handlePublish(job)}
                        disabled={publishingId === job.job_id}
                        className="rounded-md bg-emerald-600 px-2.5 py-1 text-[11px] font-medium text-white hover:bg-emerald-700 disabled:opacity-50"
                      >
                        {publishingId === job.job_id ? 'Publishing…' : 'Publish to agent'}
                      </button>
                    )}
                    {(job.status === 'queued' || job.status === 'running') && (
                      <button
                        type="button"
                        onClick={() => void cancelJob(job.job_id)}
                        className="rounded-md p-1 text-[#9CA3AF] hover:bg-gray-100 hover:text-rose-600"
                        aria-label="Cancel job"
                      >
                        <X className="size-3.5" />
                      </button>
                    )}
                  </div>
                </div>
                {(job.fine_tuned_model || job.error_message) && (
                  <p className="mt-1 truncate font-mono text-[10px] text-[#9CA3AF]">
                    {job.error_message
                      ? `error: ${job.error_message}`
                      : `model: ${job.fine_tuned_model}`}
                  </p>
                )}
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}

/**
 * Wave 9 (ui-ux-velion-gap.md §19): Embed tab.
 *
 * Lets an operator turn the agent into a public chat bubble that loads
 * on any third-party site via a single `<script>` tag. Widget JS lives
 * at `velion/public/embed.js`; public API at `/api/embed/{agentId}/*`.
 */
function EmbedTab({
  agent,
  agentName,
}: {
  agent: PersistedAgent | null;
  agentName: string;
}): ReactElement {
  const {
    publicEnabled,
    publicSecret,
    isWorking,
    error,
    enable,
    disable,
    rotateSecret,
    embedSnippet,
  } = useAgentEmbed(agent);
  const [copied, setCopied] = useState<boolean>(false);

  const copy = useCallback(async (): Promise<void> => {
    if (!embedSnippet) return;
    try {
      await navigator.clipboard.writeText(embedSnippet);
      setCopied(true);
      setTimeout(() => setCopied(false), 1800);
    } catch {
      /* clipboard may be blocked; the textarea is still selectable */
    }
  }, [embedSnippet]);

  if (!agent) {
    return (
      <div className="flex flex-col space-y-4 p-5">
        <div className="rounded-xl border border-[#E5E7EB] bg-[#F9FAFB] p-5 text-center">
          <Share2 className="mx-auto size-5 text-[#9CA3AF]" />
          <p className="mt-2 text-[13px] font-medium text-[#374151]">
            Save the agent first
          </p>
          <p className="mt-1 text-[12px] text-[#6B7280]">
            The embed widget needs a saved agent id + a generated public
            secret. Save once, then come back to enable.
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="flex flex-col space-y-6 p-5">
      <div>
        <label className="text-[13px] font-medium text-[#111827]">
          Public embed widget
        </label>
        <p className="mt-1 text-[12px] text-[#6B7280]">
          Paste the snippet into any site to expose {agentName} as a
          floating chat bubble. Visitors are anonymous; conversations
          persist per browser via the supplied secret.
        </p>
      </div>

      {!publicEnabled ? (
        <div className="rounded-xl border border-dashed border-[#D1D5DB] bg-[#F9FAFB] p-5 text-center">
          <Share2 className="mx-auto size-5 text-[#9CA3AF]" />
          <p className="mt-2 text-[13px] font-medium text-[#374151]">
            Embed widget is off
          </p>
          <p className="mt-1 text-[12px] text-[#6B7280]">
            Click below to generate a secret + enable the public
            surface. You can rotate or disable anytime.
          </p>
          <button
            type="button"
            onClick={() => void enable()}
            disabled={isWorking}
            className="mt-3 rounded-lg bg-[#111827] px-3 py-1.5 text-[12px] font-medium text-white hover:bg-gray-800 disabled:opacity-50"
          >
            {isWorking ? 'Enabling…' : 'Enable embed widget'}
          </button>
        </div>
      ) : (
        <div className="space-y-4">
          <div className="rounded-xl border border-[#E5E7EB] bg-white p-3">
            <div className="flex items-center justify-between mb-2">
              <span className="text-[11px] font-semibold uppercase tracking-wide text-[#9CA3AF]">
                Embed snippet
              </span>
              <button
                type="button"
                onClick={() => void copy()}
                className="inline-flex items-center gap-1 rounded-md bg-[#F3F4F6] px-2 py-0.5 text-[11px] font-medium text-[#374151] hover:bg-[#E5E7EB]"
              >
                {copied ? <Check className="size-3" /> : <Copy className="size-3" />}
                {copied ? 'Copied' : 'Copy'}
              </button>
            </div>
            <textarea
              readOnly
              value={embedSnippet}
              rows={7}
              className="w-full resize-none rounded-lg border border-[#E5E7EB] bg-[#F9FAFB] p-3 font-mono text-[11px] leading-relaxed text-[#374151] outline-none"
              onFocus={(e) => e.target.select()}
            />
          </div>

          <div className="rounded-xl border border-[#E5E7EB] bg-white p-3">
            <p className="text-[11px] font-semibold uppercase tracking-wide text-[#9CA3AF]">
              Public secret
            </p>
            <p className="mt-1 font-mono text-[11px] text-[#374151] break-all">
              {publicSecret ?? '—'}
            </p>
            <p className="mt-2 text-[11px] text-[#6B7280]">
              Rotating drops any already-deployed snippets immediately.
              Use if you suspect the secret has been scraped from a
              public site.
            </p>
            <div className="mt-3 flex items-center gap-2">
              <button
                type="button"
                onClick={() => void rotateSecret()}
                disabled={isWorking}
                className="inline-flex items-center gap-1.5 rounded-md bg-[#F3F4F6] px-2.5 py-1 text-[11px] font-medium text-[#374151] hover:bg-[#E5E7EB] disabled:opacity-50"
              >
                <RefreshCcw className="size-3" />
                Rotate secret
              </button>
              <button
                type="button"
                onClick={() => void disable()}
                disabled={isWorking}
                className="inline-flex items-center gap-1.5 rounded-md bg-rose-50 px-2.5 py-1 text-[11px] font-medium text-rose-700 hover:bg-rose-100 disabled:opacity-50"
              >
                <X className="size-3" />
                Disable
              </button>
            </div>
          </div>
        </div>
      )}

      {error && (
        <p className="rounded-lg bg-rose-50 px-3 py-2 text-[11px] text-rose-700">
          {error}
        </p>
      )}
    </div>
  );
}
