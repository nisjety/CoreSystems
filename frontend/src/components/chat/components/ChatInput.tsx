'use client';

import { useState, useRef, useEffect } from 'react';
import {
  Plus, ArrowUp, Lightbulb, Telescope, Globe, Mic,
  SlidersHorizontal, Clock, Crosshair, X, Wand2,
  Bot, Sparkles,
} from 'lucide-react';
import { m, AnimatePresence } from 'framer-motion';
import { useI18n } from '../hooks/i18n';
import { getDefaultModel } from './ModelSelector';

const cn = (...classes: (string | undefined | null | false)[]): string =>
  classes.filter(Boolean).join(' ');

interface Attachment {
  id: string;
  url: string;
  name: string;
}

// Types
interface ChatInputProps {
  message: string;
  setMessage: (message: string) => void;
  onSubmit: (e: React.FormEvent) => void;
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
  const effectiveSelectedModel = selectedModel || getDefaultModel(t);

  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [isRecording, setIsRecording] = useState(false);
  const [attachments, setAttachments] = useState<Attachment[]>([]);

  const hasContent = message.trim().length > 0;
  const composerDisabled = disabled || isRecording;
  const submitDisabled = !hasContent || disabled || isRecording || isLoading || isTyping;
  // Auto-resize textarea
  useEffect(() => {
    const el = textareaRef.current;
    if (el) {
      el.style.height = 'auto';
      el.style.height = `${Math.min(el.scrollHeight, 128)}px`;
    }
  }, [message]);

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      onSubmit(e as unknown as React.FormEvent);
    }
  };

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    onSubmit(e);
  };

  const handleFileChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = Array.from(e.target.files ?? []);
    const next = files.map(file => ({
      id: `${Date.now()}-${file.name}`,
      url: URL.createObjectURL(file),
      name: file.name,
    }));
    setAttachments(prev => [...prev, ...next]);
    e.target.value = '';
  };

  const removeAttachment = (id: string) => {
    setAttachments(prev => {
      const att = prev.find(a => a.id === id);
      if (att) URL.revokeObjectURL(att.url);
      return prev.filter(a => a.id !== id);
    });
  };

  return (
    <div className="w-full">
      {/* Hidden file input */}
      <input
        ref={fileInputRef}
        type="file"
        accept="image/*"
        multiple
        onChange={handleFileChange}
        className="sr-only"
      />

      {/* ── Model / Agent bar (floats above the card) ─── */}
      <div className="flex items-center justify-between mb-3 px-1">
        <div className="flex items-center gap-2">
          {/* Model selector — subtle white pill */}
          <button
            type="button"
            className="flex items-center gap-2 px-3.5 py-2 rounded-full bg-white/80 backdrop-blur-sm border border-black/[0.07] shadow-sm text-[13px] font-medium text-[#1a1a1a] hover:bg-white hover:shadow-md transition-all"
          >
            <Bot size={14} className="shrink-0 text-[#666]" />
            <span>{effectiveSelectedModel}</span>
          </button>

          {/* Create Agent — dark pill (inspired by "Use in Editor") */}
          <button
            type="button"
            className="flex items-center gap-2 px-4 py-2 rounded-full bg-[#2a2a2a] text-white text-[13px] font-medium shadow-md hover:bg-[#1a1a1a] active:scale-[0.97] transition-all"
          >
            <Sparkles size={13} className="shrink-0" />
            <span>Create agent</span>
          </button>
        </div>

        {/* Settings + History — clean icon pills */}
        <div className="flex items-center gap-1.5">
          <button
            type="button"
            className="w-9 h-9 flex items-center justify-center rounded-full bg-white/80 backdrop-blur-sm border border-black/[0.07] shadow-sm text-[#888] hover:text-[#333] hover:bg-white transition-all"
            aria-label="Settings"
          >
            <SlidersHorizontal size={14} />
          </button>
          <button
            type="button"
            className="w-9 h-9 flex items-center justify-center rounded-full bg-white/80 backdrop-blur-sm border border-black/[0.07] shadow-sm text-[#888] hover:text-[#333] hover:bg-white transition-all"
            aria-label="History"
          >
            <Clock size={14} />
          </button>
        </div>
      </div>

      <form onSubmit={handleSubmit} className="w-full bg-white rounded-[20px] shadow-[0_4px_24px_rgba(0,0,0,0.10)] overflow-hidden">
        {/* ── Attachments ───────────────────────────────── */}
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
                    {/* eslint-disable-next-line @next/next/no-img-element */}
                    <img src={att.url} alt={att.name} className="w-full h-full object-cover" />
                    <button
                      type="button"
                      onClick={() => removeAttachment(att.id)}
                      className="absolute top-1.5 right-1.5 w-5 h-5 rounded-full bg-white/90 border border-black/10 shadow-sm flex items-center justify-center hover:bg-white transition-colors"
                      aria-label="Remove attachment"
                    >
                      <X size={10} className="text-[#555]" />
                    </button>
                  </m.div>
                ))}
              </div>
              {/* AI enhance button, anchored top-right of card */}
              <button
                type="button"
                className="absolute top-4 right-4 p-2 rounded-xl text-[#6E56CF] hover:bg-violet-50 transition-colors"
                aria-label="AI enhance"
              >
                <Wand2 size={16} />
              </button>
            </m.div>
          )}
        </AnimatePresence>

        {/* ── Textarea ──────────────────────────────────── */}
        <div className="px-4 pt-4 pb-3">
          <textarea
            id="aquatiq-chat-input"
            ref={textareaRef}
            value={message}
            onChange={e => setMessage(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder={placeholder}
            disabled={composerDisabled}
            aria-busy={isLoading || isTyping}
            rows={1}
            className="w-full resize-none bg-transparent outline-none text-[15px] text-[#1a1a1a] placeholder:text-[#c0bab5] leading-relaxed disabled:opacity-50"
            style={{ minHeight: '26px', maxHeight: '128px' }}
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

        {/* ── Bottom toolbar ────────────────────────────── */}
        <div className="flex items-center justify-between px-3 pb-3 pt-1">
          {/* Left: action icons */}
          <div className="flex items-center gap-0.5">
            {/* + add files — circular badge + label style */}
            <button
              type="button"
              onClick={() => fileInputRef.current?.click()}
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

            <button
              type="button"
              disabled={disabled}
              className="p-[7px] rounded-lg text-[#666] hover:text-[#1a1a1a] hover:bg-black/5 transition-colors disabled:opacity-40"
              aria-label="Suggestions"
            >
              <Lightbulb size={15} />
            </button>

            <button
              type="button"
              disabled={disabled}
              className="p-[7px] rounded-lg text-[#666] hover:text-[#1a1a1a] hover:bg-black/5 transition-colors disabled:opacity-40"
              aria-label="Deep search"
            >
              <Telescope size={15} />
            </button>

            <button
              type="button"
              disabled={disabled}
              className="p-[7px] rounded-lg text-[#666] hover:text-[#1a1a1a] hover:bg-black/5 transition-colors disabled:opacity-40"
              aria-label="Browse web"
            >
              <Globe size={15} />
            </button>
          </div>

          {/* Right: mic + send */}
          <div className="flex items-center gap-1.5">
            <button
              type="button"
              disabled={disabled}
              className="p-[7px] rounded-lg text-[#666] hover:text-[#1a1a1a] hover:bg-black/5 transition-colors disabled:opacity-40"
              aria-label="Focus"
            >
              <Crosshair size={15} />
            </button>

            <button
              type="button"
              disabled={disabled}
              onClick={() => setIsRecording(r => !r)}
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
      </form>
    </div>
  );
};
  
