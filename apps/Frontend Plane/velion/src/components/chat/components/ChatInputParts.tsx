'use client';

import React, { useState } from 'react';
import Image from 'next/image';
import {
  Plus, ArrowUp, Lightbulb, Telescope, Globe, Mic, AudioWaveform,
  SlidersHorizontal, Clock, X, Wand2,
  Sparkles, Calendar, Upload, FileText, User,
  Zap, Sparkle, Image as ImageIcon,
} from 'lucide-react';
import { m, AnimatePresence } from 'framer-motion';
import { ModelSelector } from './ModelSelector';
import { ChatSettingsModal } from './ChatSettingsModal';
import { ChatHistoryModal } from './ChatHistoryModal';
import type { Attachment, EntityToken, ResponseMode } from './use-chat-input-state';
import type { AcItem } from './AutocompleteDropdown';

const cn = (...classes: (string | undefined | null | false)[]): string =>
  classes.filter(Boolean).join(' ');

// ── Autocomplete helpers ──────────────────────────────────────────────────

const DAY_ENTRIES = [
  { name: 'Monday',    dayIndex: 1 },
  { name: 'Tuesday',   dayIndex: 2 },
  { name: 'Wednesday', dayIndex: 3 },
  { name: 'Thursday',  dayIndex: 4 },
  { name: 'Friday',    dayIndex: 5 },
  { name: 'Saturday',  dayIndex: 6 },
  { name: 'Sunday',    dayIndex: 0 },
];

export function getUpcomingDates(dayIndex: number): AcItem[] {
  const today = new Date();
  const items: AcItem[] = [];
  const d = new Date(today);
  while (items.length < 2) {
    d.setDate(d.getDate() + 1);
    if (d.getDay() === dayIndex) {
      const label = d.toLocaleDateString('en-US', {
        weekday: 'short',
        month: 'short',
        day: 'numeric',
      });
      items.push({ id: `date-${label}`, icon: <Calendar size={16} />, label, meta: 'date' });
    }
  }
  return items;
}

interface SlashCmd extends AcItem {
  category: string;
  action: string;
}

export const SLASH_COMMANDS: SlashCmd[] = [
  {
    id: 'cmd-file',
    icon: <Upload size={16} />,
    label: 'File upload',
    category: 'Attachment',
    action: 'file',
    meta: 'slash',
  },
  {
    // U2-15 closed (velion ui-ux-velion-gap.md §10): real image generation
    // via Azure `gpt-image-1`. The chat input intercepts a message that
    // starts with `/image ` (or selects the slash command), calls
    // /api/ai/images, and inserts the resulting PNG into the conversation.
    id: 'cmd-image',
    icon: <ImageIcon size={16} />,
    label: 'Generate image',
    category: 'AI',
    action: 'image',
    meta: 'slash',
  },
];

export type TriggerCtx =
  | { type: 'date'; dayIndex: number; start: number; rawLen: number }
  | { type: 'slash'; query: string; start: number; rawLen: number }
  | { type: 'person'; query: string; start: number; rawLen: number };

export function detectTrigger(text: string, pos: number): TriggerCtx | null {
  const before = text.slice(0, pos);

  const atM = before.match(/@(\w*)$/);
  if (atM) {
    return {
      type: 'person',
      query: atM[1].toLowerCase(),
      start: pos - atM[0].length,
      rawLen: atM[0].length,
    };
  }

  const slashM = before.match(/\/(\w*)$/);
  if (slashM) {
    return {
      type: 'slash',
      query: slashM[1].toLowerCase(),
      start: pos - slashM[0].length,
      rawLen: slashM[0].length,
    };
  }

  const wordM = before.match(/\b([A-Za-z]{3,})$/);
  if (wordM) {
    const lower = wordM[1].toLowerCase();
    const entry = DAY_ENTRIES.find(e => e.name.toLowerCase().startsWith(lower));
    if (entry) {
      return {
        type: 'date',
        dayIndex: entry.dayIndex,
        start: pos - wordM[1].length,
        rawLen: wordM[1].length,
      };
    }
  }

  return null;
}

// ── Entity token overlay ─────────────────────────────────────────────────

export function Overlay({ msg, entities }: { msg: string; entities: EntityToken[] }) {
  if (!entities.length) {
    return <span className="text-[#1a1a1a] whitespace-pre-wrap">{msg || '\u200b'}</span>;
  }

  const parts: React.ReactNode[] = [];
  let pos = 0;

  const valid = entities
    .filter(
      e =>
        e.start >= 0 &&
        e.start + e.text.length <= msg.length &&
        msg.slice(e.start, e.start + e.text.length) === e.text
    )
    .sort((a, b) => a.start - b.start);

  for (const ent of valid) {
    if (ent.start > pos) {
      parts.push(
        <span key={`t${pos}`} className="text-[#1a1a1a] whitespace-pre-wrap">
          {msg.slice(pos, ent.start)}
        </span>
      );
    }
    const end = ent.start + ent.text.length;
    if (ent.kind === 'date') {
      parts.push(
        <span key={`e${ent.start}`} className="text-blue-500 font-medium">
          {ent.text}
        </span>
      );
    } else if (ent.kind === 'person') {
      parts.push(
        <span key={`e${ent.start}`} className="text-violet-600 font-medium">
          {ent.text}
        </span>
      );
    } else {
      parts.push(
        <span
          key={`e${ent.start}`}
          className="inline-flex items-center gap-1 text-[#333] font-medium align-baseline"
        >
          <span className="inline-block w-3.5 h-3.5 rounded-full border border-[#999] shrink-0" />
          {ent.text}
        </span>
      );
    }
    pos = end;
  }

  if (pos < msg.length) {
    parts.push(
      <span key={`t${pos}`} className="text-[#1a1a1a] whitespace-pre-wrap">
        {msg.slice(pos)}
      </span>
    );
  }

  return <>{parts}</>;
}

// ── Constants ─────────────────────────────────────────────────────────────

export const SUGGESTIONS = [
  'Summarize my recent activity and key tasks',
  'Help me draft a document about ',
  'Find information related to ',
  'Schedule a meeting with ',
] as const;

export const RESPONSE_MODES: Array<{ value: ResponseMode; label: string; announcement: string }> = [
  { value: 'auto',  label: 'Auto',           announcement: 'Auto mode' },
  { value: 'quick', label: 'Quick response',  announcement: 'Quick response activated' },
  { value: 'deep',  label: 'Deep research',   announcement: 'Deep research mode' },
];

export const MODE_ICONS: Record<ResponseMode, React.ReactNode> = {
  auto:  <Sparkle  size={16} />,
  quick: <Zap      size={16} />,
  deep:  <Lightbulb size={16} />,
};

// ── Tooltip ───────────────────────────────────────────────────────────────

interface TooltipProps { label: string; children: React.ReactNode }
export const Tooltip: React.FC<TooltipProps> = ({ label, children }) => {
  const [visible, setVisible] = useState(false);
  return (
    <div className="relative" onMouseEnter={() => setVisible(true)} onMouseLeave={() => setVisible(false)}>
      {children}
      <AnimatePresence>
        {visible && (
          <m.div
            initial={{ opacity: 0, y: 4, scale: 0.96 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 4, scale: 0.96 }}
            transition={{ duration: 0.12, ease: 'easeOut' }}
            className="absolute bottom-full left-1/2 -translate-x-1/2 mb-2.5 pointer-events-none z-50"
          >
            <div className="px-3.5 py-2 rounded-[12px] bg-[#1a1a1a] text-white text-[13px] font-semibold whitespace-nowrap shadow-xl">
              {label}
            </div>
            <div className="absolute top-full left-1/2 -translate-x-1/2 overflow-hidden w-4 h-2">
              <div className="absolute -top-1 left-1/2 -translate-x-1/2 w-2.5 h-2.5 bg-[#1a1a1a] rotate-45" />
            </div>
          </m.div>
        )}
      </AnimatePresence>
    </div>
  );
};

// ── SplitText ─────────────────────────────────────────────────────────────

export const SplitText: React.FC<{ text: string }> = ({ text }) => (
  <>
    {text.split('').map((char, i) => (
      <m.span
        key={`${char}-${i}`}
        initial={{ opacity: 0, y: 6 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ delay: i * 0.025, type: 'spring', stiffness: 500, damping: 35 }}
        className="inline-block"
      >
        {char === ' ' ? '\u00a0' : char}
      </m.span>
    ))}
  </>
);

// ── AttachmentPreview ─────────────────────────────────────────────────────

interface AttachmentPreviewProps {
  attachments: Attachment[];
  onRemove: (id: string) => void;
  onEnhance: () => void;
}

export const AttachmentPreview: React.FC<AttachmentPreviewProps> = ({ attachments, onRemove, onEnhance }) => (
  <AnimatePresence>
    {attachments.length > 0 && (
      <m.div
        initial={{ opacity: 0, height: 0 }}
        animate={{ opacity: 1, height: 'auto' }}
        exit={{ opacity: 0, height: 0 }}
        className="relative px-4 pt-4 pb-2 overflow-hidden"
      >
        <div className="flex items-start gap-2">
          {attachments.map(att => (
            <m.div
              key={att.id}
              initial={{ opacity: 0, scale: 0.8 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.8 }}
              className="relative w-[88px] h-[88px] rounded-2xl overflow-hidden shrink-0 ring-1 ring-black/8"
            >
              <Image src={att.url} alt={att.name} className="w-full h-full object-cover" width={88} height={88} />
              <button
                type="button"
                onClick={() => onRemove(att.id)}
                className="absolute top-1.5 right-1.5 w-5 h-5 rounded-full bg-white/90 border border-black/10 shadow-sm flex items-center justify-center hover:bg-white transition-colors"
                aria-label="Remove attachment"
              >
                <X size={10} className="text-[#555]" />
              </button>
            </m.div>
          ))}
        </div>
        <button
          type="button"
          onClick={onEnhance}
          className="absolute top-4 right-4 p-2 rounded-xl text-[#6E56CF] hover:bg-violet-50 transition-colors"
          aria-label="AI enhance"
        >
          <Wand2 size={16} />
        </button>
      </m.div>
    )}
  </AnimatePresence>
);

// ── ModelAgentBar ─────────────────────────────────────────────────────────

interface ModelAgentBarProps {
  localModel: string;
  onModelChange: (model: string) => void;
  onNavigateAgents: () => void;
  onAddFiles: () => void;
  onScreenshot: (file: File) => void;
}

export const ModelAgentBar: React.FC<ModelAgentBarProps> = ({
  localModel,
  onModelChange,
  onNavigateAgents,
  onAddFiles,
  onScreenshot,
}) => (
  <div className="flex items-center justify-between mb-3 px-1">
    <div className="flex items-center gap-2">
      <ModelSelector
        value={localModel}
        onChange={onModelChange}
        dropDirection="up"
      />
      <button
        type="button"
        onClick={onNavigateAgents}
        className="flex items-center gap-2 px-4 py-2 rounded-[12px] bg-[#2a2a2a] text-white text-[13px] font-medium shadow-md hover:bg-[#1a1a1a] active:scale-[0.97] transition-all"
      >
        <Sparkles size={13} className="shrink-0" />
        <span>Create agent</span>
      </button>
    </div>
    <div className="flex items-center gap-1.5">
      <ChatHistoryModal
        trigger={
          <button
            type="button"
            className="w-9 h-9 flex items-center justify-center rounded-[12px] bg-white/80 backdrop-blur-sm border border-black/[0.07] shadow-sm text-[#888] hover:text-[#333] hover:bg-white transition-all"
            aria-label="History"
          >
            <Clock size={14} />
          </button>
        }
      />
      <ChatSettingsModal
        trigger={
          <button
            type="button"
            className="w-9 h-9 flex items-center justify-center rounded-[12px] bg-white/80 backdrop-blur-sm border border-black/[0.07] shadow-sm text-[#888] hover:text-[#333] hover:bg-white transition-all"
            aria-label="Settings"
          >
            <SlidersHorizontal size={14} />
          </button>
        }
        onAddFiles={onAddFiles}
        onScreenshot={onScreenshot}
      />
    </div>
  </div>
);

// ── InputToolbar ──────────────────────────────────────────────────────────

interface InputToolbarProps {
  disabled: boolean;
  onAddFiles: () => void;
  showSuggestions: boolean;
  onToggleSuggestions: () => void;
  deepSearch: boolean;
  onToggleDeepSearch: () => void;
  browseWeb: boolean;
  onToggleBrowseWeb: () => void;
  responseMode: ResponseMode;
  onResponseMode: (mode: ResponseMode) => void;
  isRecording: boolean;
  onToggleRecording: () => void;
  /** U2-15 follow-up: opens the realtime voice conversation modal. */
  onOpenVoiceMode: () => void;
  submitDisabled: boolean;
  isLoading: boolean;
}

export const InputToolbar: React.FC<InputToolbarProps> = ({
  disabled,
  onAddFiles,
  showSuggestions,
  onToggleSuggestions,
  deepSearch,
  onToggleDeepSearch,
  browseWeb,
  onToggleBrowseWeb,
  responseMode,
  onResponseMode,
  isRecording,
  onToggleRecording,
  onOpenVoiceMode,
  submitDisabled,
  isLoading,
}) => (
  <div className="flex items-center justify-between px-3 pb-3 pt-1">
    <div className="flex items-center gap-0.5">
      <button
        type="button"
        onClick={onAddFiles}
        disabled={disabled}
        className="flex items-center gap-1.5 text-[#888] hover:text-[#333] transition-colors disabled:opacity-40 group"
        aria-label="Add files"
      >
        <span className="w-7 h-7 rounded-full bg-black/6 group-hover:bg-black/10 flex items-center justify-center transition-colors">
          <Plus size={14} />
        </span>
        <span className="text-[13px]">add files</span>
      </button>

      <div className="w-px h-4 bg-black/10 mx-1" />

      <Tooltip label="Suggestions">
        <button
          type="button"
          disabled={disabled}
          onClick={onToggleSuggestions}
          className={cn(
            'p-[7px] rounded-lg transition-colors disabled:opacity-40',
            showSuggestions
              ? 'text-amber-500 bg-amber-50 hover:bg-amber-100'
              : 'text-[#666] hover:text-[#1a1a1a] hover:bg-black/5'
          )}
          aria-label="Suggestions"
          aria-pressed={showSuggestions}
        >
          <Lightbulb size={15} />
        </button>
      </Tooltip>

      {/*
        U2-7 + U2-8 closed (ui-ux-velion-gap.md §10 + §12):
        Both Deep Search and Browse Web now flow real flags into the
        Rust gateway. Deep Search routes to `/v1/research` (multi-step
        plan→fetch→synthesize loop). Browse Web injects Brave search
        snippets into `/v1/invoke` as grounding context.
      */}
      <Tooltip label="Deep search — multi-step research via /v1/research">
        <button
          type="button"
          disabled={disabled}
          onClick={onToggleDeepSearch}
          className={cn(
            'p-[7px] rounded-lg transition-colors disabled:opacity-40',
            deepSearch
              ? 'text-blue-500 bg-blue-50 hover:bg-blue-100'
              : 'text-[#666] hover:text-[#1a1a1a] hover:bg-black/5',
          )}
          aria-label="Deep search"
          aria-pressed={deepSearch}
        >
          <Telescope size={15} />
        </button>
      </Tooltip>

      {/* U2-7 + U2-16 closed: Browse Web is now real. The gateway's
          /v1/invoke handler runs a Brave search when this flips on and
          prepends results as grounding context. Falls back gracefully
          to no-grounding when BRAVE_API_KEY isn't configured. */}
      <Tooltip label="Browse web — live Brave search injected as grounding">
        <button
          type="button"
          disabled={disabled}
          onClick={onToggleBrowseWeb}
          className={cn(
            'flex items-center gap-1.5 px-3 py-[7px] rounded-lg transition-colors disabled:opacity-40',
            browseWeb
              ? 'bg-blue-50 hover:bg-blue-100'
              : 'hover:bg-blue-50/60',
          )}
          aria-label="Browse web"
          aria-pressed={browseWeb}
        >
          <Globe size={15} className="text-blue-500" />
          <span className="text-[13px] font-semibold text-blue-500">Search</span>
        </button>
      </Tooltip>
    </div>

    <div className="flex items-center gap-1.5">
      <div className="flex items-center rounded-xl bg-black/6 p-[3px] gap-0.5">
        {RESPONSE_MODES.map(mode => (
          <Tooltip key={mode.value} label={mode.label}>
            <button
              type="button"
              disabled={disabled}
              onClick={() => onResponseMode(mode.value)}
              className={cn(
                'p-[7px] rounded-[9px] transition-all disabled:opacity-40',
                responseMode === mode.value
                  ? 'bg-white shadow-sm text-[#1a1a1a]'
                  : 'text-[#999] hover:text-[#444]'
              )}
              aria-label={mode.label}
              aria-pressed={responseMode === mode.value}
            >
              {MODE_ICONS[mode.value]}
            </button>
          </Tooltip>
        ))}
      </div>

      {/* U2-15 follow-up (ui-ux-velion-gap.md §10): Voice mode opens a real
          WebSocket session to model-gateway's /v1/ai/realtime — turn-based
          STT → chat → TTS conversation. The neighbouring mic button is a
          one-shot voice-to-text shortcut and remains unchanged. */}
      <Tooltip label="Voice mode — live conversation">
        <button
          type="button"
          disabled={disabled}
          onClick={onOpenVoiceMode}
          className="p-[7px] rounded-lg text-[#666] hover:text-[#1a1a1a] hover:bg-black/5 transition-colors disabled:opacity-40"
          aria-label="Open voice mode"
        >
          <AudioWaveform size={15} />
        </button>
      </Tooltip>

      <button
        type="button"
        disabled={disabled}
        onClick={onToggleRecording}
        className={cn(
          'p-[7px] rounded-lg transition-colors disabled:opacity-40',
          isRecording
            ? 'text-orange-500 bg-orange-50 hover:bg-orange-100'
            : 'text-[#666] hover:text-[#1a1a1a] hover:bg-black/5'
        )}
        aria-label="Voice input"
        aria-pressed={isRecording}
      >
        <Mic size={15} />
      </button>

      <button
        type="submit"
        disabled={submitDisabled}
        className={cn(
          'w-11 h-11 flex items-center justify-center rounded-[14px] transition-all duration-200',
          !submitDisabled
            ? 'bg-[#1a1a1a] text-white hover:bg-[#333] shadow-md'
            : 'bg-black/6 text-[#ccc] cursor-not-allowed'
        )}
        aria-label="Send message"
      >
        {isLoading ? (
          <m.div
            animate={{ rotate: 360 }}
            transition={{ duration: 1, repeat: Infinity, ease: 'linear' }}
            className="w-4 h-4 border-2 border-white/30 border-t-white rounded-full"
          />
        ) : (
          <ArrowUp size={17} strokeWidth={2.5} />
        )}
      </button>
    </div>
  </div>
);
