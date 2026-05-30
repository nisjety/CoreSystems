'use client';

import { useEffect, useRef } from 'react';
import { Loader2 } from 'lucide-react';

interface StreamingAnswerProps {
  text: string;
  isStreaming: boolean;
}

export function StreamingAnswer({ text, isStreaming }: StreamingAnswerProps) {
  const cursorRef = useRef<HTMLSpanElement>(null);

  // Auto-scroll cursor into view while streaming
  useEffect(() => {
    cursorRef.current?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
  }, [text]);

  if (!text && isStreaming) {
    return (
      <div className="flex items-center gap-2 py-6 text-[#A09890]">
        <Loader2 size={14} strokeWidth={1.5} className="animate-spin" />
        <span className="font-inter text-sm">Tenker…</span>
      </div>
    );
  }

  if (!text) return null;

  // Render paragraphs
  const paragraphs = text.split('\n\n').filter(Boolean);

  return (
    <div className="space-y-4">
      {paragraphs.map((para, i) => (
        <p
          key={para}
          className="font-inter text-[15px] leading-[1.75] text-[#2B2B2B]"
        >
          {para}
          {isStreaming && i === paragraphs.length - 1 && (
            <span
              ref={cursorRef}
              aria-hidden
              className="ml-0.5 inline-block h-[1.1em] w-px animate-pulse bg-[#2B2B2B] align-text-bottom"
            />
          )}
        </p>
      ))}
    </div>
  );
}
