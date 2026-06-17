'use client';

import React from 'react';
import { X, Send, Sparkles } from 'lucide-react';
import { cn } from '@/lib/utils';

interface AIChatModalProps {
  isOpen: boolean;
  onOpenChange: (open: boolean) => void;
  title?: string;
  description?: string;
  trigger?: React.ReactNode;
}

interface ModalContentProps {
  isOpen: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description: string;
  messages: Array<{ role: 'user' | 'assistant'; content: string }>;
  input: string;
  setInput: (value: string) => void;
  isLoading: boolean;
  onSendMessage: (e: React.FormEvent) => Promise<void>;
  messagesEndRef: React.RefObject<HTMLDivElement | null>;
}

function ModalContent({
  isOpen,
  onOpenChange,
  title,
  description,
  messages,
  input,
  setInput,
  isLoading,
  onSendMessage,
  messagesEndRef,
}: ModalContentProps) {
  return (
    <>
      {/* Overlay */}
      <div
        className="fixed inset-0 z-40 bg-black/10 transition-opacity"
        onClick={() => onOpenChange(false)}
        aria-hidden="true"
      />

      {/* Modal Panel - Right Side */}
      <div
        className={cn(
          'fixed right-0 top-0 h-screen w-full max-w-[400px] z-50',
          'border-l border-[#E9EBF2] bg-white shadow-[-20px_0_80px_rgba(17,17,17,0.10)]',
          'flex flex-col transition-transform duration-300 ease-out',
          isOpen ? 'translate-x-0' : 'translate-x-full'
        )}
      >
        {/* Header */}
        <div className="flex-shrink-0 border-b border-[#EBEBEB] px-5 py-4 flex items-center justify-between bg-white">
          <div className="flex items-center gap-2.5">
            <div className="h-7 w-7 rounded-full bg-[#F5F5F5] flex items-center justify-center">
              <Sparkles className="h-3.5 w-3.5 text-[#111111]" />
            </div>
            <div>
              <h2 className="text-[13px] font-bold text-[#111111] leading-tight">{title}</h2>
              <p className="text-[11px] text-[#AAAAAA] leading-tight">{description}</p>
            </div>
          </div>
          <button
            onClick={() => onOpenChange(false)}
            className={cn(
              'flex h-7 w-7 items-center justify-center rounded-full',
              'text-[#AAAAAA] hover:text-[#111111] hover:bg-[#F5F5F5] transition-colors',
              'focus:outline-none'
            )}
            aria-label="Close"
          >
            <X className="h-3.5 w-3.5" />
          </button>
        </div>

        {/* Messages Area */}
        <div className="flex-1 overflow-y-auto px-5 py-5 space-y-3">
          {messages.length === 0 ? (
            <div className="flex flex-col items-center justify-center h-full gap-3">
              <div className="h-10 w-10 rounded-full bg-[#F5F5F5] flex items-center justify-center">
                <Sparkles className="h-5 w-5 text-[#111111]" />
              </div>
              <div className="text-center">
                <p className="text-[13px] font-bold text-[#111111]">How can I help?</p>
                <p className="text-[11px] text-[#AAAAAA] mt-1">
                  Ask me anything about what you&apos;re working on.
                </p>
              </div>
            </div>
          ) : (
            <>
              {messages.map((message) => (
                <div
                  key={message.role + message.content.slice(0, 30)}
                  className={cn(
                    'flex gap-2.5 animate-in fade-in-0 slide-in-from-bottom-2 duration-300',
                    message.role === 'user' ? 'justify-end' : 'justify-start'
                  )}
                >
                  {message.role === 'assistant' && (
                    <div className="h-7 w-7 rounded-full bg-[#F5F5F5] flex items-center justify-center flex-shrink-0 mt-0.5">
                      <Sparkles className="h-3.5 w-3.5 text-[#111111]" />
                    </div>
                  )}
                  <div
                    className={cn(
                      'max-w-[300px] px-3.5 py-2.5 rounded-2xl text-[13px] break-words leading-relaxed',
                      message.role === 'user'
                        ? 'bg-[#111111] text-white rounded-br-sm'
                        : 'bg-[#F5F5F5] text-[#111111] rounded-bl-sm'
                    )}
                  >
                    {message.content}
                  </div>
                </div>
              ))}
              {isLoading && (
                <div className="flex gap-2.5 items-center">
                  <div className="h-7 w-7 rounded-full bg-[#F5F5F5] flex items-center justify-center flex-shrink-0">
                    <Sparkles className="h-3.5 w-3.5 text-[#111111]" />
                  </div>
                  <div className="px-3.5 py-2.5 rounded-2xl rounded-bl-sm bg-[#F5F5F5]">
                    <div className="flex gap-1">
                      <div className="h-1.5 w-1.5 rounded-full bg-[#AAAAAA] animate-bounce" />
                      <div className="h-1.5 w-1.5 rounded-full bg-[#AAAAAA] animate-bounce" style={{ animationDelay: '0.2s' }} />
                      <div className="h-1.5 w-1.5 rounded-full bg-[#AAAAAA] animate-bounce" style={{ animationDelay: '0.4s' }} />
                    </div>
                  </div>
                </div>
              )}
              <div ref={messagesEndRef} />
            </>
          )}
        </div>

        {/* Input Area */}
        <div className="flex-shrink-0 border-t border-[#EBEBEB] px-5 py-4 bg-white">
          <form onSubmit={onSendMessage} className="flex gap-2 items-center">
            <input
              type="text"
              value={input}
              onChange={(e) => setInput(e.target.value)}
              placeholder="Ask something..."
              disabled={isLoading}
              className={cn(
                'flex-1 px-3.5 py-2.5 rounded-xl border border-[#E9EBF2]',
                'bg-[#F9F9F9] text-[13px] text-[#111111] placeholder-[#AAAAAA]',
                'focus:outline-none focus:border-[#CCCCCC] focus:bg-white transition-colors',
                'disabled:opacity-50 disabled:cursor-not-allowed'
              )}
            />
            <button
              type="submit"
              disabled={isLoading || !input.trim()}
              className={cn(
                'flex h-9 w-9 items-center justify-center rounded-xl',
                'text-white transition-all duration-200 focus:outline-none flex-shrink-0',
                input.trim() && !isLoading
                  ? 'bg-[#111111] hover:bg-[#333333] cursor-pointer'
                  : 'bg-[#E8E8E8] cursor-not-allowed'
              )}
              aria-label="Send message"
            >
              <Send className="h-3.5 w-3.5" />
            </button>
          </form>
        </div>
      </div>
    </>
  );
}

export function AIChatModal({
  isOpen,
  onOpenChange,
  title = 'AI Assistant',
  description = 'Get help with your current page',
  trigger,
}: AIChatModalProps) {
  const [hoveredParent, setHoveredParent] = React.useState(false);

  React.useEffect(() => {
    if (hoveredParent && !isOpen) {
      onOpenChange(true);
    } else if (!hoveredParent && isOpen) {
      onOpenChange(false);
    }
  }, [hoveredParent, isOpen, onOpenChange]);
  const [messages, setMessages] = React.useState<Array<{ role: 'user' | 'assistant'; content: string }>>([]);
  const [input, setInput] = React.useState('');
  const [isLoading, setIsLoading] = React.useState(false);
  const messagesEndRef = React.useRef<HTMLDivElement>(null);

  const scrollToBottom = () => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  };

  React.useEffect(() => {
    scrollToBottom();
  }, [messages]);

  const handleSendMessage = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!input.trim()) return;

    const userMessage = input.trim();
    setInput('');
    setMessages((prev) => [...prev, { role: 'user', content: userMessage }]);
    setIsLoading(true);

    try {
      const response = await fetch('/api/chat/send', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        credentials: 'include',
        body: JSON.stringify({ content: userMessage }),
      });

      if (!response.ok) {
        const payload = await response.json().catch(() => null) as { message?: string } | null;
        throw new Error(payload?.message ?? `Chat request failed (${response.status})`);
      }

      const assistantMessage = await response.json() as { content?: string };
      setMessages((prev) => [
        ...prev,
        {
          role: 'assistant',
          content: assistantMessage.content ?? 'The assistant returned an empty response.',
        },
      ]);
    } catch (error) {
      setMessages((prev) => [
        ...prev,
        {
          role: 'assistant',
          content: error instanceof Error ? error.message : 'Unable to reach the assistant.',
        },
      ]);
    } finally {
      setIsLoading(false);
    }
  };

  if (!isOpen && !trigger) return null;

  if (trigger) {
    return (
      <div onMouseEnter={() => setHoveredParent(true)} onMouseLeave={() => setHoveredParent(false)}>
        {trigger}
        {isOpen && (
          <ModalContent
            isOpen={isOpen}
            onOpenChange={onOpenChange}
            title={title}
            description={description}
            messages={messages}
            input={input}
            setInput={setInput}
            isLoading={isLoading}
            onSendMessage={handleSendMessage}
            messagesEndRef={messagesEndRef}
          />
        )}
      </div>
    );
  }

  return (
    <ModalContent
      isOpen={isOpen}
      onOpenChange={onOpenChange}
      title={title}
      description={description}
      messages={messages}
      input={input}
      setInput={setInput}
      isLoading={isLoading}
      onSendMessage={handleSendMessage}
      messagesEndRef={messagesEndRef}
    />
  );
}
