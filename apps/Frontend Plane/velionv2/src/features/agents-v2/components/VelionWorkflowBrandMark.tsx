"use client";

import type { ReactNode } from "react";
import { Bot, FileText } from "lucide-react";
import { cn } from "@/lib/utils";
import type { BrandMarkId } from "@/features/agents-v2/lib/verevon-workflow-builder-data";

export function BrandMark({
  brand,
  size = "medium",
}: {
  brand: BrandMarkId;
  size?: "small" | "medium" | "large";
}) {
  const className = cn(
    "grid place-items-center rounded-[8px] font-semibold leading-none",
    size === "small" ? "size-5 text-[11px]" : size === "large" ? "size-9 text-[18px]" : "size-8 text-[14px]",
    brand === "instagram"
      ? "bg-[radial-gradient(circle_at_30%_30%,#FFE66E_0%,#FF5C5C_34%,#C832D8_66%,#4C64FF_100%)] text-white"
      : brand === "facebook"
        ? "bg-[#1877F2] text-white"
        : brand === "linkedin"
          ? "bg-[#0A66C2] text-white"
          : brand === "sheets"
            ? "bg-[#2FBF71] text-white"
            : brand === "gemini"
              ? "bg-white text-[#4C7CFF]"
              : brand === "grok"
                ? "bg-white text-[#111111]"
                : brand === "perplexity"
                  ? "bg-white text-[#20898C]"
                  : brand === "drive"
                    ? "bg-white text-[#1EA362]"
                    : brand === "slides"
                      ? "bg-white text-[#F4B400]"
                      : brand === "docs"
                        ? "bg-white text-[#4285F4]"
                        : brand === "slack"
                          ? "bg-white text-[#36C5F0]"
                          : brand === "notion"
                            ? "bg-white text-[#111111]"
                            : "bg-white text-[#111111]",
  );

  const label: Record<BrandMarkId, ReactNode> = {
    openai: <Bot className={size === "large" ? "size-6" : "size-4"} strokeWidth={2} />,
    facebook: "f",
    instagram: "ig",
    linkedin: "in",
    sheets: <FileText className={size === "large" ? "size-6" : "size-4"} strokeWidth={2} />,
    gemini: "*",
    grok: "G",
    perplexity: "P",
    drive: "D",
    slides: "S",
    docs: "D",
    slack: "#",
    notion: "N",
  };

  return <span className={className}>{label[brand]}</span>;
}
