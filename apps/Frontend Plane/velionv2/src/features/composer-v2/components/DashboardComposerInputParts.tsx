"use client";

import Image from "next/image";
import type React from "react";
import { ArrowUp, FileText, WandSparkles, X } from "lucide-react";
import type { ComposerFile, ComposerTurn } from "@/features/dashboard-v2/lib/dashboard-composer-model";
import type { AutocompleteItem, EntityToken } from "@/features/composer-v2/lib/dashboard-composer-types";
import { TopLayerTooltip } from "@/features/shell-v2/components/TopLayerTooltip";
import { cn } from "@/lib/utils";

export function AttachmentPreview({
  attachments,
  onEnhance,
  onRemove,
}: {
  attachments: ComposerFile[];
  onEnhance: () => void;
  onRemove: (id: string) => void;
}) {
  if (attachments.length === 0) {
    return null;
  }

  return (
    <div className="velion-attachment-preview relative overflow-hidden px-4 pb-2 pt-4">
      <div className="flex items-start gap-2 overflow-x-auto pb-1">
        {attachments.map((attachment) => {
          const isImage = attachment.type.startsWith("image/") || attachment.url.startsWith("data:image");

          return (
            <div key={attachment.id} className="relative size-[88px] shrink-0 overflow-hidden rounded-2xl bg-black/[0.04] ring-1 ring-black/8 dark:bg-white/10 dark:ring-white/10">
              {isImage ? (
                <Image src={attachment.url} alt={attachment.name} width={88} height={88} unoptimized className="size-full object-cover" />
              ) : (
                <div className="flex size-full flex-col items-center justify-center gap-1 px-2 text-center text-[#777]">
                  <FileText className="size-5" />
                  <span className="line-clamp-2 text-[10px] leading-tight">{attachment.name}</span>
                </div>
              )}
              <button
                type="button"
                onClick={() => onRemove(attachment.id)}
                className="absolute right-1.5 top-1.5 flex size-5 items-center justify-center rounded-full border border-black/10 bg-white/90 shadow-sm transition-colors hover:bg-white"
                aria-label={`Remove ${attachment.name}`}
                title={`Remove ${attachment.name}`}
              >
                <X className="size-2.5 text-[#555]" />
              </button>
            </div>
          );
        })}
      </div>
      <button
        type="button"
        onClick={onEnhance}
        className="absolute right-4 top-4 rounded-xl p-2 text-[#6E56CF] transition-colors hover:bg-violet-50 dark:text-violet-300 dark:hover:bg-violet-300/10"
        aria-label="AI enhance"
        title="AI enhance"
      >
        <WandSparkles className="size-4" />
      </button>
    </div>
  );
}

export function EntityOverlay({
  entities,
  message,
}: {
  entities: EntityToken[];
  message: string;
}) {
  if (entities.length === 0) {
    return <span className="whitespace-pre-wrap">{message || "\u200b"}</span>;
  }

  const parts: React.ReactNode[] = [];
  let position = 0;
  const validEntities = entities
    .filter((entity) => entity.start >= 0 && entity.start + entity.text.length <= message.length && message.slice(entity.start, entity.start + entity.text.length) === entity.text)
    .sort((a, b) => a.start - b.start);

  for (const entity of validEntities) {
    if (entity.start > position) {
      parts.push(
        <span key={`text-${position}`} className="whitespace-pre-wrap">
          {message.slice(position, entity.start)}
        </span>,
      );
    }

    const className =
      entity.kind === "date"
        ? "font-medium text-blue-500"
        : entity.kind === "person"
          ? "font-medium text-violet-600"
          : "inline-flex items-center gap-1 font-medium text-[#333] dark:text-[#F7F8F8]";

    parts.push(
      <span key={`entity-${entity.start}`} className={className}>
        {entity.kind === "file" ? <span className="inline-block size-3.5 shrink-0 rounded-full border border-[#999]" /> : null}
        {entity.text}
      </span>,
    );
    position = entity.start + entity.text.length;
  }

  if (position < message.length) {
    parts.push(
      <span key={`text-${position}`} className="whitespace-pre-wrap">
        {message.slice(position)}
      </span>,
    );
  }

  return <>{parts}</>;
}

export function SplitText({ text }: { text: string }) {
  return (
    <span>
      {text.split("").map((char, index) => (
        <span
          key={`${char}-${index}`}
          className="velion-split-char inline-block"
          style={{ animationDelay: `${index * 25}ms` }}
        >
          {char === " " ? "\u00a0" : char}
        </span>
      ))}
    </span>
  );
}

export function AutocompleteDropdown({
  category,
  items,
  onHover,
  onSelect,
  selectedIndex,
}: {
  category: string;
  items: AutocompleteItem[];
  onHover: (index: number) => void;
  onSelect: (item: AutocompleteItem) => void;
  selectedIndex: number;
}) {
  return (
    <div className="velion-popover velion-popover-up min-w-[220px] overflow-hidden rounded-2xl border border-black/[0.05] bg-white shadow-[0_8px_40px_rgba(0,0,0,0.13)] dark:border-white/10 dark:bg-[#141516]">
      <div className="px-4 pb-1 pt-3">
        <span className="text-[11px] font-semibold uppercase tracking-wider text-[#bbb]">{category}</span>
      </div>
      <div className="pb-2">
        {items.map((item, index) => (
          <button
            key={item.id}
            type="button"
            onMouseDown={(event) => {
              event.preventDefault();
              onSelect(item);
            }}
            onMouseEnter={() => onHover(index)}
            className={cn(
              "flex w-full items-center gap-3 px-4 py-2.5 text-left text-[14px] text-[#1a1a1a] transition-colors dark:text-[#F7F8F8]",
              index === selectedIndex ? "bg-black/[0.04] dark:bg-white/10" : "hover:bg-black/[0.03] dark:hover:bg-white/10",
            )}
          >
            <span className="shrink-0 text-[#666] dark:text-[#D3D7DE]">{item.icon}</span>
            <span className="font-medium">{item.label}</span>
          </button>
        ))}
      </div>
    </div>
  );
}

export function TurnReceipt({ turn }: { turn: ComposerTurn }) {
  return (
    <div className="velion-composer-turn-receipt velion-fade-up mt-3 rounded-[18px] border border-black/[0.05] bg-white/80 p-3 text-[12px] text-[#666] shadow-[0_10px_28px_rgba(0,0,0,0.04)] backdrop-blur-sm dark:border-[#2A2C31] dark:bg-[#141516]/85 dark:text-[#AEB4C0]">
      <div className="flex items-start gap-2">
        <span className="mt-0.5 grid size-7 shrink-0 place-items-center rounded-full bg-[#111111] text-white">
          <ArrowUp className="size-3.5" />
        </span>
        <div className="min-w-0 flex-1">
          <p className="line-clamp-2 font-medium text-[#2E2E2E]">{turn.body}</p>
          <p className="mt-1 text-[#7A756F]">
            {turn.model} · {turn.responseMode} {turn.browseWeb ? "· web" : ""} {turn.deepSearch ? "· deep search" : ""}
          </p>
          {turn.files.length > 0 ? <p className="mt-1 text-[#7A756F]">{turn.files.length} vedlegg lagt til.</p> : null}
        </div>
      </div>
    </div>
  );
}

export function IconChip({
  active,
  label,
  onClick,
  children,
}: {
  active?: boolean;
  label: string;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <TopLayerTooltip label={label} placement="top">
      <button
        type="button"
        aria-label={label}
        aria-pressed={active}
        onClick={onClick}
        className={cn(
          "flex size-9 items-center justify-center rounded-[12px] border border-black/[0.07] bg-white/80 text-[#888] shadow-sm backdrop-blur-sm transition-all hover:bg-white hover:text-[#333]",
          active ? "bg-white text-[#1A1A1A] ring-1 ring-black/[0.08]" : "",
        )}
      >
        {children}
      </button>
    </TopLayerTooltip>
  );
}

export function ToolbarIcon({
  active,
  label,
  onClick,
  children,
}: {
  active?: boolean;
  label: string;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <TopLayerTooltip label={label} placement="top">
      <button
        type="button"
        aria-label={label}
        aria-pressed={active}
        onClick={onClick}
        className={cn(
          "rounded-lg p-[7px] text-[#777] transition-colors hover:bg-black/5 hover:text-[#1a1a1a]",
          active ? "bg-white text-[#1a1a1a] shadow-sm" : "",
        )}
      >
        {children}
      </button>
    </TopLayerTooltip>
  );
}
