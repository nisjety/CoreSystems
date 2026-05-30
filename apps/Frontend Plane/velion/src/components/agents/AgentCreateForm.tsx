'use client';

import { useState, type FormEvent, type ReactElement } from 'react';
import Link from 'next/link';
import { useRouter } from 'next/navigation';
import { ArrowLeft, Loader2, Sparkles, WandSparkles } from 'lucide-react';

import {
  USE_CASE_LABELS,
  MODEL_LABELS,
  DEFAULT_MODEL,
  type AgentProfile,
  type AgentUseCase,
  type AgentStatus,
  type PersistedAgent,
  type SupportedModel,
} from './types';
import { AGENT_TOOLS } from './constants';

/**
 * Wave 9 follow-up — Create Agent form.
 *
 * Posts to `/api/agents` (Zod-validated by `createAgentSchema`). On
 * success, navigates to the new agent's workspace at
 * `/agents/{id}/playground` so the operator can immediately tune the
 * system prompt, attach knowledge, enable tools, or open the embed tab.
 *
 * Design notes:
 *   - Single-page, no wizard. Five required fields (name, use-case,
 *     model, tone, greeting); the rest live on the workspace.
 *   - System prompt has a "Generate from use-case" affordance that
 *     fills a sensible default — the operator can edit before saving.
 *   - Tools picker shows the built-in agent tools (`AGENT_TOOLS`),
 *     toggling stores the tool ids in the create payload.
 *   - Status defaults to `draft` so newly-created agents don't go live
 *     until the operator clicks Activate on the workspace.
 */

const USE_CASE_OPTIONS: ReadonlyArray<{ value: AgentUseCase; description: string }> = [
  { value: 'customer_support', description: 'Resolve customer questions across chat, email, and social.' },
  { value: 'sales', description: 'Qualify intent and guide prospects through discovery.' },
  { value: 'marketing', description: 'Run targeted campaigns and creative briefs.' },
  { value: 'hr', description: 'Answer policy questions and screen candidates.' },
  { value: 'faq', description: 'Surface knowledge-base answers with citations.' },
  { value: 'onboarding', description: 'Walk new users through activation steps.' },
  { value: 'other', description: 'Custom domain — define your own behaviour.' },
];

const TONE_OPTIONS: ReadonlyArray<{ value: string; label: string; description: string }> = [
  { value: 'professional', label: 'Professional', description: 'Calm, courteous, on-brand by default.' },
  { value: 'friendly', label: 'Friendly', description: 'Warm and conversational; first-name basis.' },
  { value: 'concise', label: 'Concise', description: 'Short, telegraphic replies; minimal filler.' },
  { value: 'detailed', label: 'Detailed', description: 'Explains reasoning, includes examples.' },
  { value: 'playful', label: 'Playful', description: 'Light humour where appropriate.' },
];

const MODEL_OPTIONS: ReadonlyArray<{ value: SupportedModel; tier: 'low' | 'high' }> = [
  { value: 'gpt-4o-mini', tier: 'low' },
  { value: 'gpt-5-mini', tier: 'high' },
  { value: 'claude-sonnet-4-5', tier: 'high' },
  { value: 'claude-opus-4-1', tier: 'high' },
];

const PROFILE_OPTIONS: ReadonlyArray<{ value: AgentProfile; label: string; description: string }> = [
  {
    value: 'chat',
    label: 'Chat',
    description: 'Clean ChatGPT-style surface for one user and their AI. The harness stays invisible.',
  },
  {
    value: 'deployed_agent',
    label: 'Deployed agent',
    description: 'Intercom/Zendesk-style bot for your customers. Operators get the run-event feed and human handoff.',
  },
];

function defaultSystemPromptFor(useCase: AgentUseCase, tone: string): string {
  const toneLine = (() => {
    switch (tone) {
      case 'friendly':
        return 'Be warm, conversational, and use the customer\'s first name when known.';
      case 'concise':
        return 'Keep replies short and telegraphic. No filler, no apology phrases.';
      case 'detailed':
        return 'Explain reasoning and include relevant examples. Cite sources when used.';
      case 'playful':
        return 'Use light humour where appropriate, but never at the customer\'s expense.';
      case 'professional':
      default:
        return 'Be calm, courteous, and on-brand. Avoid slang.';
    }
  })();

  const purposeLine = (() => {
    switch (useCase) {
      case 'customer_support':
        return 'You help customers resolve product issues. When the question is outside your knowledge, say so and offer to escalate to a human.';
      case 'sales':
        return 'You help prospective customers understand the product and qualify their fit. Ask 1–2 clarifying questions before recommending next steps.';
      case 'marketing':
        return 'You draft campaign copy, channel briefs, and creative concepts. Always specify channel and audience.';
      case 'hr':
        return 'You answer questions about company policy and benefits. For anything about an individual employee, refuse and redirect to HR.';
      case 'faq':
        return 'You answer factual questions using the knowledge base. If the answer is not in the knowledge base, say "I don\'t have that information yet" — do not guess.';
      case 'onboarding':
        return 'You walk new users through activation. Ask where they are in the process before suggesting the next step.';
      case 'other':
      default:
        return 'You are a helpful assistant. Stay on-topic and decline questions outside your scope.';
    }
  })();

  return [
    purposeLine,
    toneLine,
    '',
    'When a tool is available that can answer better than your training data (knowledge search, web fetch, etc.), prefer the tool.',
  ].join('\n');
}

interface FormState {
  name: string;
  description: string;
  useCase: AgentUseCase;
  status: AgentStatus;
  profile: AgentProfile;
  model: SupportedModel;
  temperature: number;
  systemPrompt: string;
  tone: string;
  greeting: string;
  tools: string[];
}

const INITIAL_STATE: FormState = {
  name: '',
  description: '',
  useCase: 'customer_support',
  status: 'draft',
  profile: 'chat',
  model: DEFAULT_MODEL,
  temperature: 0.4,
  systemPrompt: '',
  tone: 'professional',
  greeting: 'Hi! How can I help you today?',
  tools: ['knowledge_search'],
};

export function AgentCreateForm(): ReactElement {
  const router = useRouter();
  const [state, setState] = useState<FormState>(INITIAL_STATE);
  const [submitting, setSubmitting] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);

  const update = <K extends keyof FormState>(key: K, value: FormState[K]): void => {
    setState((prev) => ({ ...prev, [key]: value }));
  };

  const toggleTool = (id: string): void => {
    setState((prev) => ({
      ...prev,
      tools: prev.tools.includes(id) ? prev.tools.filter((t) => t !== id) : [...prev.tools, id],
    }));
  };

  const handleGeneratePrompt = (): void => {
    update('systemPrompt', defaultSystemPromptFor(state.useCase, state.tone));
  };

  const handleSubmit = async (event: FormEvent<HTMLFormElement>): Promise<void> => {
    event.preventDefault();
    setError(null);

    if (!state.name.trim()) {
      setError('Agent name is required.');
      return;
    }

    setSubmitting(true);
    try {
      // Send the system prompt the operator sees. If they never opened
      // "Generate from use-case", fall back to a sensible default so
      // the agent has something to anchor on from turn one.
      const systemPrompt = state.systemPrompt.trim()
        || defaultSystemPromptFor(state.useCase, state.tone);

      const response = await fetch('/api/agents', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          name: state.name.trim(),
          description: state.description.trim() || undefined,
          useCase: state.useCase,
          status: state.status,
          profile: state.profile,
          model: state.model,
          temperature: state.temperature,
          systemPrompt,
          tone: state.tone,
          greeting: state.greeting.trim() || undefined,
          tools: state.tools,
        }),
      });

      const payload = (await response.json().catch(() => null)) as
        | { success: boolean; data?: PersistedAgent; error?: string }
        | null;

      if (!response.ok || !payload?.success || !payload.data) {
        throw new Error(payload?.error ?? `HTTP ${response.status}`);
      }

      router.push(`/agents/${payload.data.id}/playground`);
    } catch (err: unknown) {
      const detail = err instanceof Error ? err.message : 'Failed to create agent.';
      setError(detail);
      setSubmitting(false);
    }
  };

  return (
    <div className="relative h-full overflow-y-auto bg-white text-[#23252F]">
      <div className="relative px-4 py-6 md:px-6 md:py-7 xl:px-7">
        <div className="mx-auto w-full max-w-[920px]">
          <Link
            href="/agents"
            className="inline-flex items-center gap-2 text-[12px] font-medium text-[#6B7280] hover:text-[#111111]"
          >
            <ArrowLeft className="size-3.5" />
            Back to agents
          </Link>

          <div className="mt-4 mb-6">
            <div className="inline-flex items-center gap-2 rounded-full border border-black/8 bg-[#FCFBF8]/96 px-3 py-1 text-[11px] font-semibold uppercase tracking-[0.22em] text-[#B96618] shadow-[0_2px_8px_rgba(17,24,39,0.04)]">
              <WandSparkles className="size-3" />
              New agent
            </div>
            <h1 className="mt-3 text-[28px] font-semibold leading-tight tracking-[-0.03em] text-[#1f2229] md:text-[32px]">
              Create a new agent
            </h1>
            <p className="mt-1.5 max-w-[64ch] text-[13px] leading-6 text-[#666a74]">
              Set the basics now — you can tune the system prompt, attach knowledge,
              enable tools, and connect channels from the workspace once it&apos;s live.
            </p>
          </div>

          <form onSubmit={handleSubmit} className="space-y-6">
            {/* Section: Identity */}
            <section className="rounded-[20px] border border-black/8 bg-[#FCFBF8]/96 p-5 shadow-[0_18px_40px_rgba(22,20,17,0.06)]">
              <h2 className="text-[14px] font-semibold tracking-[-0.01em] text-[#1f2229]">
                Identity
              </h2>
              <p className="mt-0.5 text-[12px] text-[#6c707b]">
                Pick a name your team will recognise.
              </p>

              <div className="mt-4 grid gap-4 md:grid-cols-2">
                <label className="block">
                  <span className="block text-[12px] font-medium text-[#374151]">
                    Agent name <span className="text-[#ef4444]">*</span>
                  </span>
                  <input
                    type="text"
                    value={state.name}
                    onChange={(e) => update('name', e.target.value)}
                    placeholder="Customer support bot"
                    maxLength={128}
                    required
                    autoFocus
                    className="mt-1 block w-full rounded-lg border border-[#e5e7eb] bg-white px-3 py-2 text-[13px] text-[#111111] outline-none placeholder:text-[#9CA3AF] focus:border-[#111111]"
                  />
                </label>

                <label className="block">
                  <span className="block text-[12px] font-medium text-[#374151]">
                    Short description
                  </span>
                  <input
                    type="text"
                    value={state.description}
                    onChange={(e) => update('description', e.target.value)}
                    placeholder="Resolves L1 product questions in chat and email."
                    maxLength={500}
                    className="mt-1 block w-full rounded-lg border border-[#e5e7eb] bg-white px-3 py-2 text-[13px] text-[#111111] outline-none placeholder:text-[#9CA3AF] focus:border-[#111111]"
                  />
                </label>
              </div>
            </section>

            {/* Section: Purpose */}
            <section className="rounded-[20px] border border-black/8 bg-[#FCFBF8]/96 p-5 shadow-[0_18px_40px_rgba(22,20,17,0.06)]">
              <h2 className="text-[14px] font-semibold tracking-[-0.01em] text-[#1f2229]">
                Purpose
              </h2>
              <p className="mt-0.5 text-[12px] text-[#6c707b]">
                What role should this agent play?
              </p>

              <div className="mt-4 grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
                {USE_CASE_OPTIONS.map((option) => {
                  const active = state.useCase === option.value;
                  return (
                    <button
                      key={option.value}
                      type="button"
                      onClick={() => update('useCase', option.value)}
                      className={`rounded-lg border p-3 text-left transition ${
                        active
                          ? 'border-[#111111] bg-white shadow-[0_2px_8px_rgba(17,24,39,0.08)]'
                          : 'border-[#e5e7eb] bg-white hover:border-[#9CA3AF]'
                      }`}
                      aria-pressed={active}
                    >
                      <span className="block text-[12px] font-semibold text-[#111111]">
                        {USE_CASE_LABELS[option.value]}
                      </span>
                      <span className="mt-0.5 block text-[11px] leading-5 text-[#6c707b]">
                        {option.description}
                      </span>
                    </button>
                  );
                })}
              </div>
            </section>

            {/* Section: Profile */}
            <section className="rounded-[20px] border border-black/8 bg-[#FCFBF8]/96 p-5 shadow-[0_18px_40px_rgba(22,20,17,0.06)]">
              <h2 className="text-[14px] font-semibold tracking-[-0.01em] text-[#1f2229]">
                Profile
              </h2>
              <p className="mt-0.5 text-[12px] text-[#6c707b]">
                How this agent is used. Drives whether operators get the run-event feed
                and human-handoff surfaces.
              </p>

              <div className="mt-4 grid gap-2 sm:grid-cols-2">
                {PROFILE_OPTIONS.map((option) => {
                  const active = state.profile === option.value;
                  return (
                    <button
                      key={option.value}
                      type="button"
                      onClick={() => update('profile', option.value)}
                      className={`rounded-lg border p-3 text-left transition ${
                        active
                          ? 'border-[#111111] bg-white shadow-[0_2px_8px_rgba(17,24,39,0.08)]'
                          : 'border-[#e5e7eb] bg-white hover:border-[#9CA3AF]'
                      }`}
                      aria-pressed={active}
                    >
                      <span className="block text-[12px] font-semibold text-[#111111]">
                        {option.label}
                      </span>
                      <span className="mt-0.5 block text-[11px] leading-5 text-[#6c707b]">
                        {option.description}
                      </span>
                    </button>
                  );
                })}
              </div>
            </section>

            {/* Section: Model + Tone */}
            <section className="rounded-[20px] border border-black/8 bg-[#FCFBF8]/96 p-5 shadow-[0_18px_40px_rgba(22,20,17,0.06)]">
              <h2 className="text-[14px] font-semibold tracking-[-0.01em] text-[#1f2229]">
                Model &amp; voice
              </h2>
              <p className="mt-0.5 text-[12px] text-[#6c707b]">
                Pick the model that powers replies and the tone it should use.
              </p>

              <div className="mt-4 grid gap-4 md:grid-cols-2">
                <label className="block">
                  <span className="block text-[12px] font-medium text-[#374151]">Model</span>
                  <select
                    value={state.model}
                    onChange={(e) => update('model', e.target.value as SupportedModel)}
                    className="mt-1 block w-full rounded-lg border border-[#e5e7eb] bg-white px-3 py-2 text-[13px] text-[#111111] outline-none focus:border-[#111111]"
                  >
                    {MODEL_OPTIONS.map((opt) => (
                      <option key={opt.value} value={opt.value}>
                        {MODEL_LABELS[opt.value]} ({opt.tier === 'high' ? 'high quality' : 'fast & cheap'})
                      </option>
                    ))}
                  </select>
                  <span className="mt-1 block text-[11px] text-[#6c707b]">
                    Lower-tier models reply faster and cost less; higher-tier reasons more carefully.
                  </span>
                </label>

                <label className="block">
                  <span className="block text-[12px] font-medium text-[#374151]">
                    Temperature: <span className="font-mono">{state.temperature.toFixed(2)}</span>
                  </span>
                  <input
                    type="range"
                    min="0"
                    max="1"
                    step="0.05"
                    value={state.temperature}
                    onChange={(e) => update('temperature', Number(e.target.value))}
                    className="mt-3 w-full accent-[#111111]"
                  />
                  <span className="mt-1 block text-[11px] text-[#6c707b]">
                    Higher = more creative, lower = more deterministic. 0.4 is a good default.
                  </span>
                </label>
              </div>

              <div className="mt-5">
                <span className="block text-[12px] font-medium text-[#374151]">Tone</span>
                <div className="mt-2 flex flex-wrap gap-2">
                  {TONE_OPTIONS.map((tone) => {
                    const active = state.tone === tone.value;
                    return (
                      <button
                        key={tone.value}
                        type="button"
                        onClick={() => update('tone', tone.value)}
                        title={tone.description}
                        className={`rounded-full border px-3 py-1 text-[12px] transition ${
                          active
                            ? 'border-[#111111] bg-[#111111] text-white'
                            : 'border-[#e5e7eb] bg-white text-[#374151] hover:border-[#9CA3AF]'
                        }`}
                        aria-pressed={active}
                      >
                        {tone.label}
                      </button>
                    );
                  })}
                </div>
              </div>
            </section>

            {/* Section: System prompt */}
            <section className="rounded-[20px] border border-black/8 bg-[#FCFBF8]/96 p-5 shadow-[0_18px_40px_rgba(22,20,17,0.06)]">
              <div className="flex items-center justify-between gap-3">
                <div>
                  <h2 className="text-[14px] font-semibold tracking-[-0.01em] text-[#1f2229]">
                    System prompt
                  </h2>
                  <p className="mt-0.5 text-[12px] text-[#6c707b]">
                    The instructions the agent receives on every turn. You can refine
                    this from the workspace later.
                  </p>
                </div>
                <button
                  type="button"
                  onClick={handleGeneratePrompt}
                  className="inline-flex items-center gap-1.5 rounded-full border border-[#e5e7eb] bg-white px-3 py-1.5 text-[11px] font-medium text-[#374151] hover:border-[#111111] hover:text-[#111111]"
                >
                  <Sparkles className="size-3" />
                  Generate from use-case
                </button>
              </div>

              <textarea
                value={state.systemPrompt}
                onChange={(e) => update('systemPrompt', e.target.value)}
                placeholder={`Leave blank to use the default for "${USE_CASE_LABELS[state.useCase]}" + tone "${state.tone}".`}
                maxLength={10000}
                rows={8}
                className="mt-3 block w-full rounded-lg border border-[#e5e7eb] bg-white px-3 py-2 font-mono text-[12px] leading-5 text-[#111111] outline-none placeholder:text-[#9CA3AF] focus:border-[#111111]"
              />
              <div className="mt-1 text-right text-[10px] text-[#9CA3AF]">
                {state.systemPrompt.length} / 10,000 characters
              </div>
            </section>

            {/* Section: Greeting */}
            <section className="rounded-[20px] border border-black/8 bg-[#FCFBF8]/96 p-5 shadow-[0_18px_40px_rgba(22,20,17,0.06)]">
              <h2 className="text-[14px] font-semibold tracking-[-0.01em] text-[#1f2229]">
                First message
              </h2>
              <p className="mt-0.5 text-[12px] text-[#6c707b]">
                The agent sends this on every new conversation. Keep it short.
              </p>
              <input
                type="text"
                value={state.greeting}
                onChange={(e) => update('greeting', e.target.value)}
                placeholder="Hi! How can I help you today?"
                maxLength={500}
                className="mt-3 block w-full rounded-lg border border-[#e5e7eb] bg-white px-3 py-2 text-[13px] text-[#111111] outline-none placeholder:text-[#9CA3AF] focus:border-[#111111]"
              />
            </section>

            {/* Section: Tools */}
            <section className="rounded-[20px] border border-black/8 bg-[#FCFBF8]/96 p-5 shadow-[0_18px_40px_rgba(22,20,17,0.06)]">
              <h2 className="text-[14px] font-semibold tracking-[-0.01em] text-[#1f2229]">
                Tools
              </h2>
              <p className="mt-0.5 text-[12px] text-[#6c707b]">
                Pick what the agent can do. You can add Data Plane skills and channel
                integrations from the workspace once created.
              </p>

              <ul className="mt-4 grid gap-2 md:grid-cols-2">
                {AGENT_TOOLS.map((tool) => {
                  const active = state.tools.includes(tool.id);
                  const Icon = tool.icon;
                  return (
                    <li key={tool.id}>
                      <button
                        type="button"
                        onClick={() => toggleTool(tool.id)}
                        className={`flex w-full items-start gap-3 rounded-lg border p-3 text-left transition ${
                          active
                            ? 'border-[#111111] bg-white shadow-[0_2px_8px_rgba(17,24,39,0.08)]'
                            : 'border-[#e5e7eb] bg-white hover:border-[#9CA3AF]'
                        }`}
                        aria-pressed={active}
                      >
                        <span
                          className={`mt-0.5 inline-flex size-7 shrink-0 items-center justify-center rounded-md ${
                            active ? 'bg-[#111111] text-white' : 'bg-[#f4f4f5] text-[#374151]'
                          }`}
                        >
                          <Icon className="size-3.5" />
                        </span>
                        <span className="min-w-0 flex-1">
                          <span className="block text-[12px] font-semibold text-[#111111]">
                            {tool.name}
                          </span>
                          <span className="mt-0.5 block text-[11px] leading-5 text-[#6c707b]">
                            {tool.description}
                          </span>
                        </span>
                      </button>
                    </li>
                  );
                })}
              </ul>
            </section>

            {/* Section: Status */}
            <section className="rounded-[20px] border border-black/8 bg-[#FCFBF8]/96 p-5 shadow-[0_18px_40px_rgba(22,20,17,0.06)]">
              <h2 className="text-[14px] font-semibold tracking-[-0.01em] text-[#1f2229]">
                Initial status
              </h2>
              <p className="mt-0.5 text-[12px] text-[#6c707b]">
                Drafts are visible to your team but never reachable from the public embed.
              </p>

              <div className="mt-3 flex flex-wrap gap-2">
                {(['draft', 'active', 'inactive'] as const).map((value) => {
                  const active = state.status === value;
                  return (
                    <button
                      key={value}
                      type="button"
                      onClick={() => update('status', value)}
                      className={`rounded-full border px-3 py-1 text-[12px] capitalize transition ${
                        active
                          ? 'border-[#111111] bg-[#111111] text-white'
                          : 'border-[#e5e7eb] bg-white text-[#374151] hover:border-[#9CA3AF]'
                      }`}
                      aria-pressed={active}
                    >
                      {value}
                    </button>
                  );
                })}
              </div>
            </section>

            {error ? (
              <div
                role="alert"
                className="rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-[12px] text-red-700"
              >
                {error}
              </div>
            ) : null}

            <div className="sticky bottom-0 flex items-center justify-between gap-3 rounded-[16px] border border-black/8 bg-white/95 px-4 py-3 shadow-[0_-8px_24px_rgba(22,20,17,0.04)] backdrop-blur">
              <span className="text-[11px] text-[#6c707b]">
                You can change every field after the agent is created.
              </span>
              <div className="flex items-center gap-2">
                <Link
                  href="/agents"
                  className="rounded-full border border-[#e5e7eb] bg-white px-4 py-2 text-[12px] font-medium text-[#374151] hover:border-[#9CA3AF]"
                >
                  Cancel
                </Link>
                <button
                  type="submit"
                  disabled={submitting || !state.name.trim()}
                  className="inline-flex items-center gap-2 rounded-full bg-[#111111] px-4 py-2 text-[12px] font-medium text-white transition hover:bg-[#2B2B2B] disabled:cursor-not-allowed disabled:bg-[#9CA3AF]"
                >
                  {submitting ? (
                    <>
                      <Loader2 className="size-3.5 animate-spin" />
                      Creating…
                    </>
                  ) : (
                    <>
                      <Sparkles className="size-3.5" />
                      Create agent
                    </>
                  )}
                </button>
              </div>
            </div>
          </form>
        </div>
      </div>
    </div>
  );
}
