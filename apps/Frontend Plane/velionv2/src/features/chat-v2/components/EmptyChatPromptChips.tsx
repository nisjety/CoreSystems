"use client";

import { useEffect, useRef, useState } from "react";
import {
  Briefcase,
  Code2,
  GraduationCap,
  Laptop,
  MoreHorizontal,
  Palette,
  PenLine,
  Presentation,
  WandSparkles,
  type LucideIcon,
} from "lucide-react";

type EmptyChatPrompt = {
  label: string;
  prompt: string;
  icon: LucideIcon;
};

const primaryEmptyChatPrompts: ReadonlyArray<EmptyChatPrompt> = [
  {
    label: "Create slides",
    prompt: "Create a concise slide outline for a customer support leadership update.",
    icon: Presentation,
  },
  {
    label: "Build website",
    prompt: "Build a focused website plan for a high-converting support automation page.",
    icon: Code2,
  },
  {
    label: "Develop desktop apps",
    prompt: "Plan a desktop app workflow for agents managing customer conversations.",
    icon: Laptop,
  },
  {
    label: "Design",
    prompt: "Design a refined support workflow with clear states, handoffs, and escalation paths.",
    icon: Palette,
  },
  {
    label: "Write",
    prompt: "Write a polished customer reply that is concise, helpful, and on-brand.",
    icon: PenLine,
  },
];

const overflowEmptyChatPrompts: ReadonlyArray<EmptyChatPrompt> = [
  {
    label: "Learn",
    prompt: "Teach me the most important concepts behind customer support automation.",
    icon: GraduationCap,
  },
  {
    label: "Code",
    prompt: "Help me implement a clean customer support automation feature with tests.",
    icon: Code2,
  },
  {
    label: "Career chat",
    prompt: "Help me prepare for a career conversation about customer experience leadership.",
    icon: Briefcase,
  },
  {
    label: "Verevon's choice",
    prompt: "Choose the highest-impact next task for improving our support operations.",
    icon: WandSparkles,
  },
];

export function EmptyChatPromptChips({
  onSelectPrompt,
}: {
  onSelectPrompt: (prompt: string) => void;
}) {
  const [moreOpen, setMoreOpen] = useState(false);
  const moreMenuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!moreOpen) {
      return;
    }

    const closeOnOutsidePointer = (event: PointerEvent) => {
      if (!moreMenuRef.current?.contains(event.target as Node)) {
        setMoreOpen(false);
      }
    };

    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setMoreOpen(false);
      }
    };

    document.addEventListener("pointerdown", closeOnOutsidePointer);
    document.addEventListener("keydown", closeOnEscape);

    return () => {
      document.removeEventListener("pointerdown", closeOnOutsidePointer);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [moreOpen]);

  const selectPrompt = (prompt: string) => {
    onSelectPrompt(prompt);
    setMoreOpen(false);
  };

  return (
    <div className="mt-5 flex flex-wrap items-center justify-center gap-2.5 px-2">
      {primaryEmptyChatPrompts.map(({ icon: Icon, label, prompt }) => (
        <button
          key={label}
          type="button"
          aria-label={`Use quick prompt: ${label}`}
          onClick={() => selectPrompt(prompt)}
          className="verevon-quick-chip verevon-ui-focus"
        >
          <Icon className="size-4 shrink-0 text-[#858892] dark:text-[#AEB4C0]" strokeWidth={1.9} />
          <span className="whitespace-nowrap">{label}</span>
        </button>
      ))}
      <div ref={moreMenuRef} className="relative">
        <button
          type="button"
          aria-label="Show more quick prompts"
          aria-expanded={moreOpen}
          aria-haspopup="menu"
          onClick={() => setMoreOpen((open) => !open)}
          className="verevon-quick-chip verevon-ui-focus"
          data-state={moreOpen ? "open" : undefined}
        >
          <MoreHorizontal className="size-4 shrink-0 text-[#858892] dark:text-[#AEB4C0]" strokeWidth={1.9} />
          <span className="whitespace-nowrap">More</span>
        </button>
        {moreOpen ? (
          <div
            role="menu"
            aria-label="More quick prompts"
            className="verevon-popover absolute left-1/2 top-full z-[120] mt-2 w-56 -translate-x-1/2 p-1.5"
          >
            {overflowEmptyChatPrompts.map(({ icon: Icon, label, prompt }) => (
              <button
                key={label}
                type="button"
                role="menuitem"
                onClick={() => selectPrompt(prompt)}
                className="verevon-menu-item verevon-ui-focus"
              >
                <Icon className="size-4 shrink-0 text-[#858892] dark:text-[#AEB4C0]" strokeWidth={1.9} />
                <span>{label}</span>
              </button>
            ))}
          </div>
        ) : null}
      </div>
    </div>
  );
}
