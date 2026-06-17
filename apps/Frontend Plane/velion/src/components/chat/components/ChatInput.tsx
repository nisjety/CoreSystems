'use client';

import { useRef, useEffect, useCallback, useState } from 'react';
import { useRouter } from 'next/navigation';
import { useChatInputState, type AcState, type ResponseMode } from './use-chat-input-state';
import { RealtimeVoiceModal } from './RealtimeVoiceModal';
import { m, AnimatePresence } from 'framer-motion';
import { useI18n } from '../hooks/i18n';
import { getDefaultModel } from './ModelSelector';
import { AutocompleteDropdown, type AcItem } from './AutocompleteDropdown';
import { FileText, User } from 'lucide-react';
import {
  detectTrigger,
  getUpcomingDates,
  SLASH_COMMANDS,
  SUGGESTIONS,
  RESPONSE_MODES,
  Overlay,
  SplitText,
  AttachmentPreview,
  ModelAgentBar,
  InputToolbar,
} from './ChatInputParts';

import type { SendOptions } from '@/components/chat/providers/ChatProvider';

interface ChatInputProps {
  message: string;
  setMessage: (message: string) => void;
  onSubmit: (e: React.FormEvent, options?: SendOptions) => void;
  disabled?: boolean;
  placeholder?: string;
  selectedModel?: string;
  onModelChange?: (model: string) => void;
  /** @deprecated ModelSelector handles models internally */
  models?: string[];
  context?: 'dashboard' | 'chatpage';
  isLoading?: boolean;
  isTyping?: boolean;
}

export const ChatInput: React.FC<ChatInputProps> = ({
  message,
  setMessage,
  onSubmit,
  disabled = false,
  selectedModel,
  onModelChange: _onModelChange,
  context: _context = 'dashboard',
  isLoading = false,
  isTyping = false,
  placeholder = 'How can I help you today?',
}) => {
  const { t } = useI18n();
  const router = useRouter();
  const effectiveSelectedModel = selectedModel || getDefaultModel(t);

  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const pendingFilePosRef = useRef<number | null>(null);
  const acTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const acAbortRef = useRef<AbortController | null>(null);
  const mediaRecorderRef = useRef<MediaRecorder | null>(null);
  const audioChunksRef = useRef<Blob[]>([]);
  const modeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const [state, dispatch] = useChatInputState(effectiveSelectedModel);
  const {
    localModel, deepSearch, browseWeb, responseMode, modeAnnouncement,
    showSuggestions, isRecording, attachments, acState, acIndex, entities,
  } = state;

  // U2-15 follow-up (ui-ux-velion-gap.md §10): controls the RealtimeVoiceModal
  // (live STT → chat → TTS conversation). The existing mic button is the
  // one-shot voice-to-text shortcut and stays independent of this flag.
  const [voiceModeOpen, setVoiceModeOpen] = useState(false);

  // Keep localModel in sync when the prop changes externally
  useEffect(() => {
    dispatch({ type: 'SET_MODEL', model: selectedModel || getDefaultModel(t) });
  }, [selectedModel, t]);

  const handleModelChange = useCallback((model: string) => {
    dispatch({ type: 'SET_MODEL', model });
    _onModelChange?.(model);
  }, [_onModelChange]);

  const handleResponseMode = useCallback((mode: ResponseMode) => {
    if (mode === responseMode) return;
    const found = RESPONSE_MODES.find(m => m.value === mode);
    if (found) {
      dispatch({ type: 'SET_RESPONSE_MODE', mode, announcement: found.announcement });
      if (modeTimerRef.current) clearTimeout(modeTimerRef.current);
      modeTimerRef.current = setTimeout(() => dispatch({ type: 'CLEAR_ANNOUNCEMENT' }), 3000);
    }
  }, [responseMode]);

  // Cleanup mode timer on unmount
  useEffect(() => () => { if (modeTimerRef.current) clearTimeout(modeTimerRef.current); }, []);

  const hasContent = message.trim().length > 0;
  const composerDisabled = disabled || isRecording;
  const submitDisabled = !hasContent || disabled || isRecording || isLoading || isTyping;
  const hasOverlay = entities.length > 0;

  // Auto-resize textarea
  useEffect(() => {
    const el = textareaRef.current;
    if (el) {
      el.style.height = 'auto';
      el.style.height = `${Math.min(el.scrollHeight, 128)}px`;
    }
  }, [message]);

  const updateAc = useCallback(
    (text: string, pos: number) => {
      // Cancel any in-flight fetch or pending debounce
      if (acTimerRef.current) { clearTimeout(acTimerRef.current); acTimerRef.current = null; }
      if (acAbortRef.current) { acAbortRef.current.abort(); acAbortRef.current = null; }

      const trigger = detectTrigger(text, pos);
      if (!trigger) {
        dispatch({ type: 'SET_AUTOCOMPLETE', ac: null });
        return;
      }

      if (trigger.type === 'date') {
        const items = getUpcomingDates(trigger.dayIndex);
        if (items.length) {
          dispatch({ type: 'SET_AUTOCOMPLETE', ac: {
            items,
            category: 'Schedule',
            triggerStart: trigger.start,
            triggerLen: trigger.rawLen,
          } });
        } else {
          dispatch({ type: 'SET_AUTOCOMPLETE', ac: null });
        }
      } else if (trigger.type === 'slash') {
        const { query, start, rawLen } = trigger;
        const matches = SLASH_COMMANDS.filter(
          c =>
            c.action.startsWith(query) ||
            c.label.toLowerCase().startsWith(query)
        );
        if (matches.length) {
          dispatch({ type: 'SET_AUTOCOMPLETE', ac: {
            items: matches,
            category: matches[0].category,
            triggerStart: start,
            triggerLen: rawLen,
          } });
        } else if (query.length > 0) {
          // No slash command match — search documents from backend
          acTimerRef.current = setTimeout(() => {
            const ctrl = new AbortController();
            acAbortRef.current = ctrl;
            fetch(`/api/autocomplete/documents?q=${encodeURIComponent(query)}&limit=5`, {
              signal: ctrl.signal,
            })
              .then(r => (r.ok ? r.json() : null))
              .then((data: { suggestions?: Array<{ id: string; title: string; type: string }> } | null) => {
                const list = data?.suggestions ?? [];
                if (!list.length) { dispatch({ type: 'SET_AUTOCOMPLETE', ac: null }); return; }
                const items: AcItem[] = list.map(doc => ({
                  id: doc.id,
                  icon: <FileText size={16} />,
                  label: doc.title,
                  meta: 'file',
                }));
                dispatch({ type: 'SET_AUTOCOMPLETE', ac: { items, category: 'Documents', triggerStart: start, triggerLen: rawLen } });
              })
              .catch(() => {});
          }, 180);
        } else {
          dispatch({ type: 'SET_AUTOCOMPLETE', ac: null });
        }
      } else if (trigger.type === 'person') {
        // Debounce → fetch live member suggestions
        const { query, start, rawLen } = trigger;
        acTimerRef.current = setTimeout(() => {
          const ctrl = new AbortController();
          acAbortRef.current = ctrl;
          fetch(`/api/autocomplete/members?q=${encodeURIComponent(query)}&limit=6`, {
            signal: ctrl.signal,
          })
            .then(r => (r.ok ? r.json() : null))
            .then((data: { suggestions?: Array<{ user_id: string; display_name: string; email: string }> } | null) => {
              const list = data?.suggestions ?? [];
              if (!list.length) { dispatch({ type: 'SET_AUTOCOMPLETE', ac: null }); return; }
              const items: AcItem[] = list.map(m => ({
                id: m.user_id,
                icon: <User size={16} />,
                label: m.display_name,
                meta: 'person',
              }));
              dispatch({ type: 'SET_AUTOCOMPLETE', ac: { items, category: 'People', triggerStart: start, triggerLen: rawLen } });
            })
            .catch(() => { /* aborted or network error — stay silent */ });
        }, 180);
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    []
  );

  const handleChange = useCallback(
    (e: React.ChangeEvent<HTMLTextAreaElement>) => {
      const text = e.target.value;
      const pos = e.target.selectionStart ?? text.length;
      if (modeAnnouncement) {
        dispatch({ type: 'CLEAR_ANNOUNCEMENT' });
        if (modeTimerRef.current) { clearTimeout(modeTimerRef.current); modeTimerRef.current = null; }
      }
      setMessage(text);
      updateAc(text, pos);
      // Prune stale entity tokens
      dispatch({ type: 'PRUNE_ENTITIES', message: text });
    },
    [setMessage, updateAc, modeAnnouncement]
  );

  const applySelection = useCallback(
    (item: AcItem) => {
      if (!acState) return;
      const { triggerStart, triggerLen } = acState;

      // Slash /file → open file picker, insert chip on pick
      const cmd = SLASH_COMMANDS.find(c => c.id === item.id);
      if (cmd?.action === 'file') {
        const newMsg =
          message.slice(0, triggerStart) +
          message.slice(triggerStart + triggerLen);
        setMessage(newMsg);
        dispatch({ type: 'SET_AUTOCOMPLETE', ac: null });
        pendingFilePosRef.current = triggerStart;
        fileInputRef.current?.click();
        return;
      }

      // U2-15: /image → prefill the input so the next submit triggers
      // the image-generation intercept in handleSubmit. We replace the
      // slash trigger with the canonical "/image " prefix and leave the
      // cursor at the end so the user can type their prompt.
      if (cmd?.action === 'image') {
        const before = message.slice(0, triggerStart);
        const after = message.slice(triggerStart + triggerLen);
        const replaced = `${before}/image ${after}`;
        setMessage(replaced);
        dispatch({ type: 'SET_AUTOCOMPLETE', ac: null });
        requestAnimationFrame(() => {
          const el = textareaRef.current;
          if (el) {
            const caret = before.length + '/image '.length;
            el.focus();
            el.setSelectionRange(caret, caret);
          }
        });
        return;
      }

      // Date / person / other → insert label and mark as entity
      const before = message.slice(0, triggerStart);
      const after = message.slice(triggerStart + triggerLen);
      // Person: keep the @ prefix in the inserted text
      const inserted = item.meta === 'person' ? `@${item.label}` : item.label;
      const newMsg = before + inserted + after;
      setMessage(newMsg);

      if (item.meta === 'date' || item.meta === 'person' || item.meta === 'file') {
        dispatch({ type: 'ADD_ENTITY', entity: { start: triggerStart, text: inserted, kind: item.meta } });
      }

      dispatch({ type: 'SET_AUTOCOMPLETE', ac: null });
      requestAnimationFrame(() => {
        const el = textareaRef.current;
        if (el) {
          el.focus();
          const cur = triggerStart + inserted.length;
          el.setSelectionRange(cur, cur);
        }
      });
    },
    [acState, message, setMessage]
  );

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (acState) {
        if (e.key === 'ArrowDown') {
          e.preventDefault();
          dispatch({ type: 'SET_AC_INDEX', index: Math.min(acIndex + 1, acState.items.length - 1) });
          return;
        }
        if (e.key === 'ArrowUp') {
          e.preventDefault();
          dispatch({ type: 'SET_AC_INDEX', index: Math.max(acIndex - 1, 0) });
          return;
        }
        if (e.key === 'Enter' || e.key === 'Tab') {
          e.preventDefault();
          applySelection(acState.items[acIndex]);
          return;
        }
        if (e.key === 'Escape') {
          dispatch({ type: 'SET_AUTOCOMPLETE', ac: null });
          return;
        }
      }
      if (e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault();
        onSubmit(e as unknown as React.FormEvent);
      }
    },
    [acState, acIndex, applySelection, onSubmit]
  );

  const handleSubmit = useCallback(async (e: React.FormEvent) => {
    e.preventDefault();
    if (acState) return;

    // U2-15: slash-command intercept — `/image <prompt>` calls the real
    // Azure gpt-image-1 backend via /api/ai/images, then drops the
    // resulting PNG into the conversation as a synthetic attachment so
    // the LLM sees it the same way as a user-uploaded image. Use the
    // submitted message verbatim (less the `/image ` prefix) as the
    // prompt; the rest of the chat flow proceeds normally.
    const imagePrefix = '/image ';
    if (message.trimStart().toLowerCase().startsWith(imagePrefix)) {
      const prompt = message.trimStart().slice(imagePrefix.length).trim();
      if (prompt) {
        try {
          const res = await fetch('/api/ai/images', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            credentials: 'include',
            body: JSON.stringify({ prompt, size: '1024x1024', quality: 'auto' }),
          });
          if (res.ok) {
            const data = (await res.json()) as { data_url?: string };
            if (data.data_url) {
              dispatch({
                type: 'ADD_ATTACHMENTS',
                items: [
                  {
                    id: `img-${Date.now()}`,
                    name: `${prompt.slice(0, 40)}.png`,
                    url: data.data_url,
                  },
                ],
              });
              setMessage(`Generated image for: "${prompt}"`);
            }
          } else {
            const body = await res.json().catch(() => ({}))
            setMessage(
              `Failed to generate image: ${typeof body.error === 'string' ? body.error : `HTTP ${res.status}`}`,
            );
          }
        } catch (err) {
          setMessage(
            `Image generation error: ${err instanceof Error ? err.message : 'unknown'}`,
          );
        }
        // Fall through to onSubmit so the chat shows the user's intent
        // and the image attachment in the same turn.
      }
    }

    // Upload any pending attachments and collect their URLs
    let attachmentUrls: string[] | undefined;
    if (attachments.length > 0) {
      try {
        const fd = new FormData();
        await Promise.all(
          attachments.map(async (att) => {
            const res = await fetch(att.url);
            const blob = await res.blob();
            fd.append('files', blob, att.name);
          })
        );
        const up = await fetch('/api/chat/upload', { method: 'POST', body: fd });
        if (up.ok) {
          const { uploads } = await up.json() as { uploads: Array<{ url: string }> };
          attachmentUrls = uploads.map(u => u.url).filter(Boolean);
        }
      } catch {
        // Non-fatal — send without attachments if upload fails
      }
    }

    // U2-8 closed: when the Deep Search toggle is on, force responseMode
    // to 'deep' so the proxy routes through /v1/research. The composer
    // still has a separate auto/quick/deep selector — the toggle is the
    // explicit "use research, not chat" override.
    const effectiveResponseMode = deepSearch ? 'deep' : responseMode

    onSubmit(e, {
      model: localModel,
      responseMode: effectiveResponseMode,
      browseWeb,
      attachmentUrls,
    });

    // Clear attachments after successful submit
    dispatch({ type: 'CLEAR_ATTACHMENTS' });
  }, [acState, attachments, browseWeb, deepSearch, dispatch, localModel, message, onSubmit, responseMode, setMessage]);

  const handleFileChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const files = Array.from(e.target.files ?? []);
      if (!files.length) return;

      if (pendingFilePosRef.current !== null) {
        // Insert inline chip from slash /file command
        const file = files[0];
        const pos = pendingFilePosRef.current;
        pendingFilePosRef.current = null;
        const chipText = file.name;
        const newMsg = message.slice(0, pos) + chipText + message.slice(pos);
        setMessage(newMsg);
        dispatch({ type: 'ADD_ENTITY', entity: { start: pos, text: chipText, kind: 'file' } });
        e.target.value = '';
        return;
      }

      // Regular image attachment
      const next = files.map(file => ({
        id: `${Date.now()}-${file.name}`,
        url: URL.createObjectURL(file),
        name: file.name,
      }));
      dispatch({ type: 'ADD_ATTACHMENTS', items: next });
      e.target.value = '';
    },
    [message, setMessage]
  );

  const removeAttachment = (id: string) => {
    dispatch({ type: 'REMOVE_ATTACHMENT', id });
  };

  const toggleRecording = useCallback(async () => {
    if (isRecording) {
      mediaRecorderRef.current?.stop();
      return;
    }
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      audioChunksRef.current = [];
      const recorder = new MediaRecorder(stream);
      mediaRecorderRef.current = recorder;

      recorder.ondataavailable = (e) => {
        if (e.data.size > 0) audioChunksRef.current.push(e.data);
      };

      recorder.onstop = async () => {
        stream.getTracks().forEach(t => t.stop());
        const blob = new Blob(audioChunksRef.current, { type: 'audio/webm' });
        audioChunksRef.current = [];
        dispatch({ type: 'SET_RECORDING', on: false });
        try {
          const fd = new FormData();
          fd.append('audio', blob, 'recording.webm');
          const res = await fetch('/api/audio/transcribe', { method: 'POST', body: fd });
          if (res.ok) {
            const { text } = await res.json() as { text: string };
            if (text) setMessage(text);
          }
        } catch { /* non-fatal */ }
      };

      recorder.onerror = () => {
        stream.getTracks().forEach(t => t.stop());
        dispatch({ type: 'SET_RECORDING', on: false });
      };

      recorder.start();
      dispatch({ type: 'SET_RECORDING', on: true });
    } catch {
      dispatch({ type: 'SET_RECORDING', on: false });
    }
  }, [isRecording, setMessage]);

  const handleScreenshot = useCallback((file: File) => {
    const att = {
      id: `${Date.now()}-${file.name}`,
      url: URL.createObjectURL(file),
      name: file.name,
    };
    dispatch({ type: 'ADD_ATTACHMENTS', items: [att] });
  }, []);

  const handleEnhance = useCallback(() => {
    if (!attachments.length) return;
    const names = attachments.map(a => a.name).join(', ');
    const base = message.trim();
    setMessage(base ? `Analyze the attached file(s) (${names}) and ${base}` : `Describe and analyze the attached file(s): ${names}`);
  }, [attachments, message, setMessage]);

  return (
    <div className="relative w-full">
      {/* Hidden file input — accepts all files for slash /file, images otherwise */}
      <input
        ref={fileInputRef}
        type="file"
        accept="*/*"
        multiple
        onChange={handleFileChange}
        className="sr-only"
      />

      <ModelAgentBar
        localModel={localModel}
        onModelChange={handleModelChange}
        onNavigateAgents={() => router.push('/agents')}
        onAddFiles={() => fileInputRef.current?.click()}
        onScreenshot={handleScreenshot}
      />

      {/* Wrapper for autocomplete dropdown positioning */}
      <div className="relative">
        {/* ── Suggestions popover ──────────────────────────────── */}
        <AnimatePresence>
          {showSuggestions && (
            <m.div
              initial={{ opacity: 0, y: 4 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: 4 }}
              className="absolute bottom-full left-0 mb-2 z-50 w-72 rounded-2xl bg-white shadow-[0_8px_32px_rgba(0,0,0,0.12)] border border-black/6 p-2"
            >
              <p className="px-2 py-1.5 text-[11px] font-medium text-[#999] uppercase tracking-wide">Suggestions</p>
              {SUGGESTIONS.map((s, i) => (
                <button
                  key={s}
                  type="button"
                  onClick={() => {
                    setMessage(s);
                    dispatch({ type: 'HIDE_SUGGESTIONS' });
                    requestAnimationFrame(() => textareaRef.current?.focus());
                  }}
                  className="w-full text-left px-3 py-2.5 rounded-xl text-[13px] text-[#333] hover:bg-black/5 transition-colors"
                >
                  {s}
                </button>
              ))}
            </m.div>
          )}
        </AnimatePresence>

        <AnimatePresence>
          {acState && (
            <div className="absolute bottom-full mb-2 left-4 z-50">
              <AutocompleteDropdown
                category={acState.category}
                items={acState.items}
                selectedIndex={acIndex}
                onSelect={applySelection}
                onHover={(idx: number) => dispatch({ type: 'SET_AC_INDEX', index: idx })}
              />
            </div>
          )}
        </AnimatePresence>

        <form
          onSubmit={handleSubmit}
          className="w-full bg-white rounded-[20px] shadow-[0_4px_24px_rgba(0,0,0,0.10)] overflow-hidden"
        >
          <AttachmentPreview
            attachments={attachments}
            onRemove={removeAttachment}
            onEnhance={handleEnhance}
          />

          {/* ── Textarea with highlight overlay ────────────────────── */}
          <div className="relative px-4 pt-4 pb-3">
            {/* Highlight overlay — mirrors textarea, pointer-events none */}
            {hasOverlay && (
              <div
                aria-hidden
                className="absolute inset-0 px-4 pt-4 pb-3 pointer-events-none select-none text-[15px] leading-relaxed break-words overflow-hidden"
              >
                <Overlay msg={message} entities={entities} />
              </div>
            )}

            {/* Mode announcement — split text, shows when mode is activated and no text typed */}
            <AnimatePresence>
              {modeAnnouncement && !message && (
                <m.div
                  key={modeAnnouncement}
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0, transition: { duration: 0.4 } }}
                  aria-hidden
                  className="absolute inset-0 px-4 pt-4 pb-3 flex items-start pointer-events-none select-none"
                >
                  <span className="text-[15px] text-[#c0bab5] font-normal">
                    <SplitText text={modeAnnouncement} />
                  </span>
                </m.div>
              )}
            </AnimatePresence>

            <textarea
              id="aquatiq-chat-input"
              ref={textareaRef}
              value={message}
              onChange={handleChange}
              onKeyDown={handleKeyDown}
              placeholder={modeAnnouncement ? '' : placeholder}
              disabled={composerDisabled}
              aria-busy={isLoading || isTyping}
              rows={1}
              className="w-full resize-none bg-transparent outline-none text-[15px] leading-relaxed placeholder:text-[#c0bab5] disabled:opacity-50"
              style={{
                minHeight: '26px',
                maxHeight: '128px',
                color: hasOverlay ? 'transparent' : '#1a1a1a',
                caretColor: '#1a1a1a',
              }}
            />
          </div>

          {/* Loading / typing progress bar */}
          {(isLoading || isTyping) && (
            <div className="mx-4 mb-2 h-px overflow-hidden rounded-full bg-black/6">
              <m.div
                className="h-full rounded-full bg-linear-to-r from-violet-400 via-blue-400 to-violet-400"
                animate={{ x: ['-100%', '100%'] }}
                transition={{ duration: 1.5, repeat: Infinity, ease: 'linear' }}
                style={{ width: '50%' }}
              />
            </div>
          )}

          <InputToolbar
            disabled={disabled}
            onAddFiles={() => fileInputRef.current?.click()}
            showSuggestions={showSuggestions}
            onToggleSuggestions={() => dispatch({ type: 'TOGGLE_SUGGESTIONS' })}
            deepSearch={deepSearch}
            onToggleDeepSearch={() => dispatch({ type: 'SET_DEEP_SEARCH', on: !deepSearch })}
            browseWeb={browseWeb}
            onToggleBrowseWeb={() => dispatch({ type: 'SET_BROWSE_WEB', on: !browseWeb })}
            responseMode={responseMode}
            onResponseMode={handleResponseMode}
            isRecording={isRecording}
            onToggleRecording={toggleRecording}
            onOpenVoiceMode={() => setVoiceModeOpen(true)}
            submitDisabled={submitDisabled}
            isLoading={isLoading}
          />
        </form>
      </div>

      <RealtimeVoiceModal
        open={voiceModeOpen}
        onClose={() => setVoiceModeOpen(false)}
        model={localModel}
      />
    </div>
  );
};
