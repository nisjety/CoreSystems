'use client';

import {
  useState,
  useEffect,
  useRef,
  useCallback,
  type ReactElement,
  type KeyboardEvent,
} from 'react';

interface Block {
  /** Stable per-block client-side id; not persisted across saves. */
  id: string;
  /** Indent depth — 0 = top level. */
  depth: number;
  /** Block text content. */
  content: string;
}

interface BlockOutlineEditorProps {
  /** Logseq-format markdown — bullet outline with `-` and indentation. */
  initialContent: string;
  /** Called when the operator stops typing for ~1.5s. */
  onSave: (content: string) => Promise<{ ok: boolean; error?: string }>;
  /** Read-only mode disables editing affordances. */
  readOnly?: boolean;
}

const SAVE_DEBOUNCE_MS = 1500;

function makeId(): string {
  return `blk-${Math.random().toString(36).slice(2, 9)}`;
}

function parseLogseq(markdown: string): Block[] {
  if (!markdown.trim()) {
    return [{ id: makeId(), depth: 0, content: '' }];
  }
  const lines = markdown.split('\n');
  const blocks: Block[] = [];
  for (const line of lines) {
    if (!line.trim()) continue;
    const match = line.match(/^(\s*)-\s?(.*)$/);
    if (match) {
      const indent = match[1].length;
      // Logseq uses 2-space or tab indents; normalize to 2 spaces.
      const depth = Math.floor(indent / 2);
      blocks.push({ id: makeId(), depth, content: match[2] });
    } else {
      // Non-bullet line — append to previous block as a soft-break.
      if (blocks.length > 0) {
        blocks[blocks.length - 1] = {
          ...blocks[blocks.length - 1],
          content: `${blocks[blocks.length - 1].content}\n${line}`,
        };
      } else {
        blocks.push({ id: makeId(), depth: 0, content: line });
      }
    }
  }
  return blocks.length > 0 ? blocks : [{ id: makeId(), depth: 0, content: '' }];
}

function serializeLogseq(blocks: ReadonlyArray<Block>): string {
  return blocks
    .map((b) => {
      const indent = '  '.repeat(b.depth);
      // Multi-line block content: prefix subsequent lines with indent+2 spaces.
      const lines = b.content.split('\n');
      const head = `${indent}- ${lines[0]}`;
      const rest = lines
        .slice(1)
        .map((l) => `${indent}  ${l}`)
        .join('\n');
      return rest ? `${head}\n${rest}` : head;
    })
    .join('\n');
}

/**
 * Wave 11.C-a — Logseq-style block-outline editor.
 *
 * Operator keystrokes:
 *   Enter         → split at cursor, create sibling block.
 *   Tab           → indent (max +1 from previous block).
 *   Shift+Tab     → outdent.
 *   Backspace     → at empty block, merge into previous.
 *   Cmd/Ctrl+S    → force save now.
 *
 * Persistence: debounced 1.5s after the last change, calls `onSave`
 * with the serialized Logseq-format markdown. The wiki-store-go
 * service stores it as a new version (`edit_reason='manual_edit'`).
 */
export function BlockOutlineEditor({
  initialContent,
  onSave,
  readOnly = false,
}: BlockOutlineEditorProps): ReactElement {
  const [blocks, setBlocks] = useState<Block[]>(() => parseLogseq(initialContent));
  const [saveState, setSaveState] = useState<'idle' | 'saving' | 'saved' | 'error'>('idle');
  const [saveError, setSaveError] = useState<string | null>(null);
  const debounceRef = useRef<number | null>(null);
  const blocksRef = useRef<Block[]>(blocks);
  blocksRef.current = blocks;
  const initialSerializedRef = useRef<string>(serializeLogseq(parseLogseq(initialContent)));

  const performSave = useCallback(async (): Promise<void> => {
    const serialized = serializeLogseq(blocksRef.current);
    if (serialized === initialSerializedRef.current) {
      setSaveState('idle');
      return;
    }
    setSaveState('saving');
    setSaveError(null);
    try {
      const result = await onSave(serialized);
      if (result.ok) {
        initialSerializedRef.current = serialized;
        setSaveState('saved');
        window.setTimeout(() => setSaveState('idle'), 2000);
      } else {
        setSaveState('error');
        setSaveError(result.error ?? 'Save failed');
      }
    } catch (err) {
      setSaveState('error');
      setSaveError(err instanceof Error ? err.message : 'Save failed');
    }
  }, [onSave]);

  const scheduleSave = useCallback((): void => {
    if (readOnly) return;
    if (debounceRef.current !== null) window.clearTimeout(debounceRef.current);
    debounceRef.current = window.setTimeout(() => {
      void performSave();
    }, SAVE_DEBOUNCE_MS);
  }, [performSave, readOnly]);

  useEffect(() => {
    return () => {
      if (debounceRef.current !== null) window.clearTimeout(debounceRef.current);
    };
  }, []);

  // Cmd/Ctrl+S — force save now (bypass debounce).
  useEffect(() => {
    if (readOnly) return;
    const handler = (event: globalThis.KeyboardEvent): void => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 's') {
        event.preventDefault();
        if (debounceRef.current !== null) {
          window.clearTimeout(debounceRef.current);
          debounceRef.current = null;
        }
        void performSave();
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [performSave, readOnly]);

  const updateBlock = (index: number, content: string): void => {
    setBlocks((prev) => prev.map((b, i) => (i === index ? { ...b, content } : b)));
    scheduleSave();
  };

  const handleKeyDown = (
    event: KeyboardEvent<HTMLTextAreaElement>,
    index: number,
  ): void => {
    const block = blocks[index];
    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault();
      const newBlock: Block = { id: makeId(), depth: block.depth, content: '' };
      setBlocks((prev) => [...prev.slice(0, index + 1), newBlock, ...prev.slice(index + 1)]);
      scheduleSave();
      window.requestAnimationFrame(() => {
        const next = document.querySelector<HTMLTextAreaElement>(
          `[data-block-id="${newBlock.id}"]`,
        );
        next?.focus();
      });
    } else if (event.key === 'Tab') {
      event.preventDefault();
      if (event.shiftKey) {
        if (block.depth > 0) {
          setBlocks((prev) =>
            prev.map((b, i) => (i === index ? { ...b, depth: b.depth - 1 } : b)),
          );
          scheduleSave();
        }
      } else {
        const prevDepth = index > 0 ? blocks[index - 1].depth : -1;
        if (block.depth <= prevDepth) {
          setBlocks((prev) =>
            prev.map((b, i) => (i === index ? { ...b, depth: b.depth + 1 } : b)),
          );
          scheduleSave();
        }
      }
    } else if (event.key === 'Backspace' && block.content === '' && blocks.length > 1) {
      event.preventDefault();
      setBlocks((prev) => prev.filter((_, i) => i !== index));
      scheduleSave();
      window.requestAnimationFrame(() => {
        const target = index > 0 ? blocks[index - 1] : blocks[1];
        if (target) {
          const el = document.querySelector<HTMLTextAreaElement>(
            `[data-block-id="${target.id}"]`,
          );
          el?.focus();
          if (el) {
            const length = el.value.length;
            el.setSelectionRange(length, length);
          }
        }
      });
    }
  };

  return (
    <div className="relative">
      <div className="space-y-0.5">
        {blocks.map((block, index) => (
          <div
            key={block.id}
            className="flex items-start gap-2"
            style={{ paddingLeft: `${block.depth * 18}px` }}
          >
            <span className="mt-2 inline-flex size-1.5 shrink-0 rounded-full bg-[#6B7280]" />
            <textarea
              data-block-id={block.id}
              value={block.content}
              onChange={(e) => updateBlock(index, e.target.value)}
              onKeyDown={(e) => handleKeyDown(e, index)}
              readOnly={readOnly}
              rows={Math.max(1, block.content.split('\n').length)}
              placeholder={index === 0 && blocks.length === 1 ? 'Start writing…' : ''}
              className="block min-w-0 flex-1 resize-none bg-transparent py-1 text-[13px] leading-6 text-[#111827] outline-none placeholder:text-[#9CA3AF]"
            />
          </div>
        ))}
      </div>

      <div className="pointer-events-none fixed bottom-4 right-6 z-10 flex items-center gap-2 text-[10px]">
        {saveState === 'saving' ? (
          <span className="rounded-full bg-white/95 px-2 py-1 text-[#6B7280] ring-1 ring-inset ring-[#E5E7EB]">
            Saving…
          </span>
        ) : saveState === 'saved' ? (
          <span className="rounded-full bg-emerald-50 px-2 py-1 text-emerald-700 ring-1 ring-inset ring-emerald-200">
            ✓ Saved
          </span>
        ) : saveState === 'error' ? (
          <span
            className="rounded-full bg-red-50 px-2 py-1 text-red-700 ring-1 ring-inset ring-red-200"
            title={saveError ?? ''}
          >
            ✗ Save failed
          </span>
        ) : null}
      </div>
    </div>
  );
}
