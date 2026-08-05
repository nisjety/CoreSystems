'use client';

import { useEffect, useState, useRef, useCallback } from 'react';
import { createPortal } from 'react-dom';
import { useRouter } from 'next/navigation';
import {
  Mic, AlignLeft, AlignCenter, AlignJustify, Check,
  ChevronRight, ArrowLeft,
  Paperclip, Camera, FolderPlus,
  Blocks, LayoutGrid, Briefcase,
  Loader2, Plus,
} from 'lucide-react';
import { m, AnimatePresence } from 'framer-motion';

import { useKnowledgeIntegrations } from '@/components/knowledge/hooks/useKnowledgeData'

// ── Persistence ────────────────────────────────────────────────────────────

const SETTINGS_KEY = 'chat-input-settings';

const VOICE_LANGUAGES = [
  { value: 'en-US', label: 'English (US)' },
  { value: 'en-GB', label: 'English (UK)' },
  { value: 'nb-NO', label: 'Norwegian (Bokmål)' },
  { value: 'nn-NO', label: 'Norwegian (Nynorsk)' },
  { value: 'sv-SE', label: 'Swedish' },
  { value: 'da-DK', label: 'Danish' },
  { value: 'de-DE', label: 'German' },
  { value: 'fr-FR', label: 'French' },
  { value: 'es-ES', label: 'Spanish' },
  { value: 'pt-BR', label: 'Portuguese (BR)' },
] as const;

const TONE_OPTIONS = [
  { value: 'concise',  label: 'Concise',  Icon: AlignLeft },
  { value: 'balanced', label: 'Balanced', Icon: AlignCenter },
  { value: 'detailed', label: 'Detailed', Icon: AlignJustify },
] as const;

// U2-14 (ui-ux-verevon-gap.md §10): skills are now fetched from
// agent-core's per-org `agent_skills` table via /api/skills. The
// previous SAMPLE_SKILLS array of three hardcoded names is gone —
// the picker shows real skills when agent-core is up and a clear
// "service unavailable" message when it isn't.
interface ApiSkill {
  id: string
  name: string
  description: string
  enabled?: boolean
}
interface SkillsListResponse {
  skills: ApiSkill[]
  status: 'ok' | 'service_unavailable'
  detail?: string
}

// U2-14 follow-up (ui-ux-verevon-gap.md §10): Projects are now real per-org
// rows in the Convex `projects` table. Fetched lazily via `<ProjectsList>`
// when the dropdown opens the projects view — keeps the main view zero-cost.
interface ApiProject {
  id: string
  title: string
  description?: string
  archived: boolean
  createdAt: number
  updatedAt: number
}
interface ProjectsListResponse {
  projects: ApiProject[]
  error?: string
}

// ── Types ──────────────────────────────────────────────────────────────────

interface ChatSettings {
  voiceLang: string;
  tone: 'concise' | 'balanced' | 'detailed';
}

const DEFAULT_SETTINGS: ChatSettings = { voiceLang: 'en-US', tone: 'balanced' };

function loadChatSettings(): ChatSettings {
  if (typeof window === 'undefined') return DEFAULT_SETTINGS;
  try {
    const raw = localStorage.getItem(SETTINGS_KEY);
    return raw ? { ...DEFAULT_SETTINGS, ...JSON.parse(raw) } : DEFAULT_SETTINGS;
  } catch { return DEFAULT_SETTINGS; }
}

function saveChatSettings(s: ChatSettings) {
  try { localStorage.setItem(SETTINGS_KEY, JSON.stringify(s)); } catch { /* noop */ }
}

type ModalView = 'main' | 'skills' | 'projects' | 'connectors';

// ── Sub-components ─────────────────────────────────────────────────────────

function Row({
  icon: Icon, iconColor, iconBg, label, sub, right, onClick,
}: {
  icon: React.ElementType; iconColor?: string; iconBg?: string;
  label: string; sub?: string; right?: React.ReactNode; onClick?: () => void;
}) {
  return (
    <div
      role={onClick ? 'button' : undefined}
      tabIndex={onClick ? 0 : undefined}
      onClick={onClick}
      onKeyDown={onClick ? e => (e.key === 'Enter' || e.key === ' ') && onClick() : undefined}
      className="flex items-center gap-3 px-3 py-2.5 rounded-[12px] hover:bg-[#f2f2f2] transition-colors cursor-default select-none"
    >
      {iconBg ? (
        <div className="w-7 h-7 rounded-xl flex items-center justify-center shrink-0" style={{ backgroundColor: iconBg }}>
          <Icon size={15} strokeWidth={1.8} style={{ color: iconColor }} />
        </div>
      ) : (
        <Icon size={17} strokeWidth={1.7} className="shrink-0" style={{ color: iconColor ?? '#333' }} />
      )}
      <div className="flex-1 min-w-0">
        <div className="text-[13.5px] font-medium text-[#1a1a1a] truncate">{label}</div>
        {sub && <div className="text-[11.5px] text-[#999] mt-0.5">{sub}</div>}
      </div>
      {right}
    </div>
  );
}

function BackHeader({ label, onBack }: { label: string; onBack: () => void }) {
  return (
    <>
      <button
        type="button"
        onClick={onBack}
        className="flex items-center gap-2 px-3 py-2.5 w-full rounded-[12px] hover:bg-[#f2f2f2] transition-colors text-[#888] mb-0.5"
      >
        <ArrowLeft size={15} strokeWidth={1.8} />
        <span className="text-[12.5px] font-medium">{label}</span>
      </button>
      <div className="h-px bg-black/8 mx-2 mb-1" />
    </>
  );
}

// ── Skills list (U2-14: real, backed by agent-core) ──────────────────────

function SkillsList({ onPick }: { onPick: (skill: ApiSkill) => void }) {
  const [state, setState] = useState<{
    skills: ApiSkill[]
    status: SkillsListResponse['status'] | 'loading'
    detail?: string
  }>({ skills: [], status: 'loading' })

  useEffect(() => {
    let cancelled = false
    fetch('/api/skills', { credentials: 'include', cache: 'no-store' })
      .then(async (res) => {
        if (!res.ok) {
          throw new Error(`HTTP ${res.status}`)
        }
        return (await res.json()) as SkillsListResponse
      })
      .then((data) => {
        if (cancelled) return
        setState({
          skills: data.skills ?? [],
          status: data.status,
          detail: data.detail,
        })
      })
      .catch((err: unknown) => {
        if (cancelled) return
        setState({
          skills: [],
          status: 'service_unavailable',
          detail: err instanceof Error ? err.message : 'fetch failed',
        })
      })
    return () => {
      cancelled = true
    }
  }, [])

  if (state.status === 'loading') {
    return (
      <p className="px-3 py-2.5 text-[12.5px] text-[#999]">
        Loading skills…
      </p>
    )
  }

  if (state.status === 'service_unavailable') {
    return (
      <div className="px-3 py-2.5">
        <p className="text-[12.5px] font-medium text-amber-700">
          Skills service is starting up
        </p>
        <p className="mt-0.5 text-[11px] text-[#999]">
          agent-core is unavailable. The skill registry will appear once the
          service is online.
        </p>
        {state.detail && (
          <p className="mt-1 text-[10px] text-[#bbb] font-mono break-all">
            {state.detail}
          </p>
        )}
      </div>
    )
  }

  if (state.skills.length === 0) {
    return (
      <p className="px-3 py-2.5 text-[12.5px] text-[#999]">
        No skills defined for this workspace yet. Open “Manage skills” below
        to create your first one.
      </p>
    )
  }

  return (
    <>
      {state.skills.map((skill) => (
        <button
          key={skill.id}
          type="button"
          onClick={() => onPick(skill)}
          className="w-full text-left px-3 py-2.5 rounded-[12px] hover:bg-[#f2f2f2] transition-colors"
        >
          <span className="block text-[13.5px] font-medium text-[#1a1a1a]">
            {skill.name}
          </span>
          {skill.description && (
            <span className="mt-0.5 block text-[11.5px] text-[#888] line-clamp-2">
              {skill.description}
            </span>
          )}
        </button>
      ))}
    </>
  )
}

// ── Projects list (U2-14 follow-up: real Convex backing) ─────────────────

function ProjectsList({
  onPick,
  onCreated,
}: {
  onPick: (project: ApiProject) => void
  onCreated: (project: ApiProject) => void
}) {
  const [state, setState] = useState<{
    projects: ApiProject[]
    status: 'loading' | 'ok' | 'error'
    error?: string
  }>({ projects: [], status: 'loading' })
  const [creating, setCreating] = useState(false)

  useEffect(() => {
    let cancelled = false
    fetch('/api/projects', { credentials: 'include', cache: 'no-store' })
      .then(async (res) => {
        if (!res.ok) throw new Error(`HTTP ${res.status}`)
        return (await res.json()) as ProjectsListResponse
      })
      .then((data) => {
        if (cancelled) return
        setState({
          projects: data.projects ?? [],
          status: 'ok',
          error: data.error,
        })
      })
      .catch((err: unknown) => {
        if (cancelled) return
        setState({
          projects: [],
          status: 'error',
          error: err instanceof Error ? err.message : 'fetch failed',
        })
      })
    return () => {
      cancelled = true
    }
  }, [])

  async function handleCreate() {
    const title = window.prompt('Name for the new project:')
    if (!title || !title.trim()) return
    setCreating(true)
    try {
      const res = await fetch('/api/projects', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        credentials: 'include',
        body: JSON.stringify({ title: title.trim() }),
      })
      if (!res.ok) {
        const body = await res.json().catch(() => ({}))
        throw new Error(
          typeof body.error === 'string' ? body.error : `HTTP ${res.status}`,
        )
      }
      const created = (await res.json()) as { id: string; title: string }
      onCreated({
        id: created.id,
        title: created.title,
        archived: false,
        createdAt: Date.now(),
        updatedAt: Date.now(),
      })
    } catch (err) {
      window.alert(`Failed to create project: ${err instanceof Error ? err.message : 'unknown'}`)
    } finally {
      setCreating(false)
    }
  }

  if (state.status === 'loading') {
    return (
      <p className="px-3 py-2.5 text-[12.5px] text-[#999]">Loading projects…</p>
    )
  }

  if (state.status === 'error') {
    return (
      <div className="px-3 py-2.5">
        <p className="text-[12.5px] font-medium text-amber-700">
          Projects service unavailable
        </p>
        <p className="mt-0.5 text-[11px] text-[#999]">
          {state.error ?? 'Could not reach Convex projects backend.'}
        </p>
      </div>
    )
  }

  return (
    <>
      {state.projects.length === 0 ? (
        <p className="px-3 py-2.5 text-[12.5px] text-[#999]">
          No projects yet — create your first one below.
        </p>
      ) : (
        state.projects.map((project) => (
          <button
            key={project.id}
            type="button"
            onClick={() => onPick(project)}
            className="w-full text-left px-3 py-2.5 rounded-[12px] hover:bg-[#f2f2f2] transition-colors"
          >
            <span className="block text-[13.5px] font-medium text-[#1a1a1a]">
              {project.title}
            </span>
            {project.description && (
              <span className="mt-0.5 block text-[11.5px] text-[#888] line-clamp-2">
                {project.description}
              </span>
            )}
          </button>
        ))
      )}
      <div className="h-px bg-black/8 mx-2 my-1" />
      <button
        type="button"
        onClick={handleCreate}
        disabled={creating}
        className="w-full flex items-center gap-3 px-3 py-2.5 rounded-[12px] hover:bg-[#f2f2f2] transition-colors disabled:opacity-40"
      >
        <div className="w-7 h-7 rounded-xl flex items-center justify-center shrink-0 bg-black/[0.04]">
          <Plus size={14} strokeWidth={1.8} className="text-[#555]" />
        </div>
        <span className="text-[13.5px] font-medium text-[#1a1a1a]">
          {creating ? 'Creating…' : 'Start a new project'}
        </span>
      </button>
    </>
  )
}

// ── Props ──────────────────────────────────────────────────────────────────

interface ChatSettingsModalProps {
  trigger: React.ReactNode;
  onAddFiles?: () => void;
  onScreenshot?: (file: File) => void;
}

// ── Main component ─────────────────────────────────────────────────────────

export function ChatSettingsModal({ trigger, onAddFiles, onScreenshot }: ChatSettingsModalProps) {
  const router = useRouter();
  const [isOpen, setIsOpen] = useState(false);
  const [pos, setPos] = useState<{ top: number; left: number; maxHeight: number }>({
    top: 0, left: 0, maxHeight: 460,
  });
  const wrapperRef = useRef<HTMLDivElement>(null);
  const modalRef = useRef<HTMLDivElement>(null);
  const [view, setView] = useState<ModalView>('main');
  const [settings, setSettings] = useState<ChatSettings>(() => loadChatSettings());
  const integrationsQuery = useKnowledgeIntegrations();
  const mounted = typeof window !== 'undefined';

  // Close on click-outside (trigger wrapper OR modal panel)
  useEffect(() => {
    if (!isOpen) return;
    const handleDown = (e: MouseEvent) => {
      const inWrapper = wrapperRef.current?.contains(e.target as Node);
      const inModal   = modalRef.current?.contains(e.target as Node);
      if (!inWrapper && !inModal) setIsOpen(false);
    };
    document.addEventListener('mousedown', handleDown);
    return () => document.removeEventListener('mousedown', handleDown);
  }, [isOpen]);

  const handleTriggerClick = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (!isOpen) {
      const rect = wrapperRef.current?.getBoundingClientRect();
      if (rect) {
        // Open to the RIGHT of the button, clamped to viewport
        const modalW = 256;
        const gap = 8;
        const rawLeft = rect.right + gap;
        const left = Math.min(rawLeft, window.innerWidth - modalW - gap);
        setPos({
          top:      rect.top,
          left,
          maxHeight: Math.min(460, window.innerHeight - rect.top - 16),
        });
      }
      setSettings(loadChatSettings());
      setView('main');
      setIsOpen(true);
      return;
    }
    setView('main');
    setIsOpen(false);
  };

  const update = <K extends keyof ChatSettings>(key: K, value: ChatSettings[K]) => {
    const next = { ...settings, [key]: value };
    setSettings(next);
    saveChatSettings(next);
  };

  const handleTakeScreenshot = useCallback(async () => {
    setIsOpen(false);
    try {
      const stream = await navigator.mediaDevices.getDisplayMedia({
        video: true,
        // @ts-expect-error — preferCurrentTab is a newer hint, not in all type defs
        preferCurrentTab: true,
      });
      const video = document.createElement('video');
      video.srcObject = stream;
      await new Promise<void>(resolve => { video.onloadedmetadata = () => resolve(); });
      video.play();
      await new Promise(r => setTimeout(r, 100));
      const canvas = document.createElement('canvas');
      canvas.width = video.videoWidth;
      canvas.height = video.videoHeight;
      canvas.getContext('2d')?.drawImage(video, 0, 0);
      stream.getTracks().forEach(t => t.stop());
      canvas.toBlob(blob => {
        if (blob) {
          const file = new File([blob], `screenshot-${Date.now()}.png`, { type: 'image/png' });
          onScreenshot?.(file);
        }
      }, 'image/png');
    } catch { /* user cancelled */ }
  }, [onScreenshot]);

  const currentLangLabel = VOICE_LANGUAGES.find(l => l.value === settings.voiceLang)?.label ?? settings.voiceLang;

  const slideVariants = {
    enterFromRight: { opacity: 0, x: 12 },
    enterFromLeft:  { opacity: 0, x: -12 },
    center:         { opacity: 1, x: 0 },
    exitToLeft:     { opacity: 0, x: -12 },
    exitToRight:    { opacity: 0, x: 12 },
  };

  return (
    <>
      <div ref={wrapperRef} role="presentation" onClick={handleTriggerClick} onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') e.currentTarget.click(); }}>
        {trigger}
      </div>

      {mounted && createPortal(
        <AnimatePresence>
          {isOpen && (
            <m.div
              ref={modalRef}
              initial={{ opacity: 0, x: -8, scale: 0.97 }}
              animate={{ opacity: 1, x: 0,   scale: 1 }}
              exit={{   opacity: 0, x: -8,  scale: 0.97 }}
              transition={{ type: 'spring', stiffness: 420, damping: 32 }}
              style={{
                position: 'fixed',
                top:  pos.top,
                left: pos.left,
                zIndex: 9999,
                transformOrigin: 'top left',
              }}
              className="w-[256px] rounded-[20px] bg-white shadow-[0_-4px_32px_rgba(0,0,0,0.10),0_4px_16px_rgba(0,0,0,0.05)] overflow-hidden"
            >
              <div className="overflow-y-auto" style={{ maxHeight: pos.maxHeight }}>
                <AnimatePresence mode="wait" initial={false}>

                  {/* ── Main view ──────────────────────────────────── */}
                  {view === 'main' && (
                    <m.div key="main"
                      variants={slideVariants} initial="enterFromLeft" animate="center" exit="exitToLeft"
                      transition={{ duration: 0.15 }} className="p-2"
                    >
                      <div className="flex items-center gap-3 px-3 py-2.5 rounded-[12px] hover:bg-[#f2f2f2] transition-colors">
                        <Mic size={17} strokeWidth={1.7} className="text-[#333] shrink-0" />
                        <span className="flex-1 text-[13.5px] font-medium text-[#1a1a1a]">Voice language</span>
                        <select
                          value={settings.voiceLang}
                          onChange={e => update('voiceLang', e.target.value)}
                          title={currentLangLabel}
                          className="text-[12px] text-[#888] bg-transparent border-none outline-none cursor-pointer max-w-24 truncate text-right"
                        >
                          {VOICE_LANGUAGES.map(l => <option key={l.value} value={l.value}>{l.label}</option>)}
                        </select>
                      </div>

                      {TONE_OPTIONS.map(opt => (
                        <Row key={opt.value} icon={opt.Icon} label={opt.label}
                          onClick={() => update('tone', opt.value)}
                          right={settings.tone === opt.value
                            ? <Check size={14} strokeWidth={2.5} className="text-[#1a1a1a] shrink-0" />
                            : <div className="w-[14px]" />}
                        />
                      ))}

                      <div className="h-px bg-black/8 mx-2 my-1" />

                      <Row icon={Paperclip} label="Add files or photos"
                        onClick={() => { setIsOpen(false); onAddFiles?.(); }}
                      />
                      <Row icon={Camera} label="Take a screenshot"
                        onClick={handleTakeScreenshot}
                      />
                      <Row icon={FolderPlus} label="Add to project"
                        onClick={() => setView('projects')}
                        right={<ChevronRight size={14} className="text-[#bbb] shrink-0" />}
                      />

                      <div className="h-px bg-black/8 mx-2 my-1" />

                      <Row icon={Blocks} label="Skills"
                        onClick={() => setView('skills')}
                        right={<ChevronRight size={14} className="text-[#bbb] shrink-0" />}
                      />
                      <Row icon={LayoutGrid} label="Connectors"
                        onClick={() => setView('connectors')}
                        right={<ChevronRight size={14} className="text-[#bbb] shrink-0" />}
                      />
                    </m.div>
                  )}

                  {/* ── Skills view (U2-14 — real agent-core registry) ── */}
                  {view === 'skills' && (
                    <m.div key="skills"
                      variants={slideVariants} initial="enterFromRight" animate="center" exit="exitToRight"
                      transition={{ duration: 0.15 }} className="p-2"
                    >
                      <BackHeader label="Skills" onBack={() => setView('main')} />
                      <SkillsList
                        onPick={(skill) => {
                          setIsOpen(false)
                          router.push(`/skills/${encodeURIComponent(skill.id)}`)
                        }}
                      />
                      <div className="h-px bg-black/8 mx-2 my-1" />
                      <Row icon={Briefcase} label="Manage skills"
                        onClick={() => { setIsOpen(false); router.push('/skills'); }}
                      />
                    </m.div>
                  )}

                  {/* ── Projects view (U2-14 follow-up — real Convex backing) ── */}
                  {view === 'projects' && (
                    <m.div key="projects"
                      variants={slideVariants} initial="enterFromRight" animate="center" exit="exitToRight"
                      transition={{ duration: 0.15 }} className="p-2"
                    >
                      <BackHeader label="Add to project" onBack={() => setView('main')} />
                      <ProjectsList
                        onPick={(p) => {
                          setIsOpen(false)
                          router.push(`/projects/${encodeURIComponent(p.id)}`)
                        }}
                        onCreated={(p) => {
                          setIsOpen(false)
                          router.push(`/projects/${encodeURIComponent(p.id)}`)
                        }}
                      />
                    </m.div>
                  )}

                  {/* ── Connectors view ─────────────────────────────── */}
                  {view === 'connectors' && (
                    <m.div key="connectors"
                      variants={slideVariants} initial="enterFromRight" animate="center" exit="exitToRight"
                      transition={{ duration: 0.15 }} className="p-2"
                    >
                      <BackHeader label="Connectors" onBack={() => setView('main')} />
                      {integrationsQuery.isLoading ? (
                        <div className="flex items-center gap-2 px-3 py-3 text-[12px] text-[#888]">
                          <Loader2 size={14} className="animate-spin" />
                          Loading connectors…
                        </div>
                      ) : (
                        (integrationsQuery.data?.providers ?? []).map((provider) => (
                          <div key={provider.key} className="flex items-center gap-3 px-3 py-2.5 rounded-[12px] hover:bg-[#f2f2f2] transition-colors">
                            <div className="w-7 h-7 rounded-xl flex items-center justify-center shrink-0 bg-black/[0.04]">
                              <LayoutGrid size={14} strokeWidth={1.8} className="text-[#555]" />
                            </div>
                            <div className="flex-1 min-w-0">
                              <div className="text-[13.5px] font-medium text-[#1a1a1a]">{provider.label}</div>
                              <div className="text-[11.5px] text-[#999] truncate">
                                {provider.connected
                                  ? (provider.connection?.selectedSources.join(', ') || 'Connected')
                                  : 'Available to connect'}
                              </div>
                            </div>
                            <button
                              type="button"
                              className="text-[12px] font-medium text-[#888] hover:text-[#444] transition-colors shrink-0"
                              onClick={() => {
                                setIsOpen(false);
                                router.push('/settings/integrations');
                              }}
                            >
                              {provider.connected ? 'Manage' : 'Open'}
                            </button>
                          </div>
                        ))
                      )}
                      <div className="h-px bg-black/8 mx-2 my-1" />
                      <Row icon={LayoutGrid} label="Connect more"
                        onClick={() => { setIsOpen(false); router.push('/settings/integrations'); }}
                      />
                    </m.div>
                  )}

                </AnimatePresence>
              </div>
            </m.div>
          )}
        </AnimatePresence>,
        document.body
      )}
    </>
  );
}
