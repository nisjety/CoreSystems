"use client";

import {
  MoreHorizontal,
  Smartphone,
  Sparkles,
  Zap,
  type LucideIcon,
} from "lucide-react";
import { VelionButton, VelionIconButton } from "@/components/ui/velion-ui";
import { cn } from "@/lib/utils";

export function IntegrationCard({
  Icon,
  description,
  status,
  title,
}: {
  Icon: LucideIcon;
  description: string;
  status: string;
  title: string;
}) {
  const connected = status.includes("connected");

  return (
    <article className="flex min-h-[210px] flex-col rounded-[12px] border border-[#E4E5E8] bg-white p-6 shadow-sm dark:border-[#303238] dark:bg-[#15161A]">
      <div className="flex items-start justify-between gap-4">
        <span className="grid size-12 place-items-center rounded-[10px] bg-[#F4F5F7] text-[#111111] dark:bg-[#202229] dark:text-white">
          <Icon className="size-5" />
        </span>
        <span
          className={cn(
            "rounded-full px-3 py-1 text-[12px] font-semibold",
            connected
              ? "bg-[#E9F8EF] text-[#16834A]"
              : "bg-[#F4F1EB] text-[#7A7168]",
          )}
        >
          {status}
        </span>
      </div>
      <h2 className="mt-5 text-[18px] font-semibold">{title}</h2>
      <p className="mt-3 text-[14px] leading-6 text-[#5F6673] dark:text-[#AEB4C0]">{description}</p>
      <VelionButton size="sm" radius="sm" className="mt-auto px-4 font-semibold">
        Configure
      </VelionButton>
    </article>
  );
}

export function ActionCard({
  Icon,
  color,
  enabled,
  subtitle,
  title,
}: {
  Icon: LucideIcon;
  color: string;
  enabled: boolean;
  subtitle: string;
  title: string;
}) {
  return (
    <article className="flex min-h-[260px] flex-col rounded-[12px] border border-[#E4E5E8] bg-white p-8 shadow-[0_1px_2px_rgba(16,24,40,0.04)] dark:border-[#303238] dark:bg-[#15161A]">
      <div className="flex items-start justify-between gap-4">
        <span className={cn("grid size-16 place-items-center rounded-[9px] border", color)}>
          <Icon className="size-7" />
        </span>
        <ToggleSwitch enabled={enabled} />
      </div>
      <h2 className="mt-7 text-[21px] font-semibold">{title}</h2>
      <p className="mt-2 flex items-center gap-2 text-[17px] font-medium text-[#6F747D]">
        <Zap className="size-4" />
        {subtitle}
      </p>
      <div className="mt-auto flex justify-end gap-3">
        <VelionIconButton size="lg" radius="sm" aria-label={`Open ${title} tool menu`} className="border border-[#E2E3E8] bg-white shadow-sm dark:border-[#303238] dark:bg-[#111216]">
          <MoreHorizontal className="size-5" />
        </VelionIconButton>
        <VelionButton radius="sm" className="px-7 text-[13px] font-semibold">
          Customize
        </VelionButton>
      </div>
    </article>
  );
}

export function ToggleSwitch({ enabled }: { enabled: boolean }) {
  return (
    <span className={cn("relative h-7 w-12 rounded-full transition-colors", enabled ? "bg-[#10B35A]" : "bg-[#D8DADE]")}>
      <span className={cn("absolute top-1 size-5 rounded-full bg-white shadow transition-transform", enabled ? "translate-x-[22px]" : "translate-x-1")} />
    </span>
  );
}

export function ChannelHeroCard({
  displayName,
  type,
}: {
  displayName: string;
  type: "widget" | "help";
}) {
  const widget = type === "widget";

  return (
    <article className="overflow-hidden rounded-[12px] border border-[#E4E5E8] bg-white shadow-[0_1px_2px_rgba(16,24,40,0.04)] dark:border-[#303238] dark:bg-[#15161A]">
      <div className={cn("relative h-[290px] overflow-hidden", widget ? "bg-[linear-gradient(135deg,#85DDEF,#159BD9)]" : "bg-[linear-gradient(135deg,#FFAA13,#FFF356)]")}>
        {widget ? (
          <div className="absolute bottom-0 left-1/2 h-[250px] w-[330px] -translate-x-1/2 rounded-t-[18px] border border-[#E1E2E6] bg-white shadow-[0_14px_34px_rgba(31,35,42,0.12)]">
            <div className="flex h-12 items-center gap-3 px-5 text-[12px] font-semibold">
              <span className="grid size-8 place-items-center rounded-full bg-[#111111] text-white"><Sparkles className="size-4" /></span>
              {displayName}
            </div>
            <div className="ml-5 mt-3 w-max rounded-full bg-[#F4F4F5] px-4 py-2 text-[12px]">Hi! What can I help you with?</div>
          </div>
        ) : (
          <div className="absolute left-1/2 top-12 h-[246px] w-[620px] -translate-x-1/2 rounded-t-[18px] bg-white px-10 pt-16 shadow-[0_14px_34px_rgba(31,35,42,0.12)]">
            <div className="absolute left-6 top-5 flex gap-2">
              <span className="size-3 rounded-full bg-[#FF3B30]" />
              <span className="size-3 rounded-full bg-[#FFCC00]" />
              <span className="size-3 rounded-full bg-[#34C759]" />
            </div>
            <h3 className="text-center text-[24px] font-semibold">How can we help you today?</h3>
            <div className="mt-8 flex h-20 items-center rounded-[16px] border border-[#E1E2E6] px-7 text-[17px] text-[#B1B3B9]">Ask a question…</div>
          </div>
        )}
      </div>
      <div className="p-7">
        <div className="flex items-start justify-between gap-4">
          <div>
            <h2 className="text-[21px] font-semibold">{widget ? "Chat widget" : "Help page"}</h2>
            <p className="mt-3 max-w-[650px] text-[17px] leading-7 text-[#555B65] dark:text-[#AEB4C0]">
              {widget ? "Add a floating chat window to your site." : "ChatGPT-style help page, deployed standalone or under a path on your site (/help)."}
            </p>
          </div>
          {widget ? <ToggleSwitch enabled /> : null}
        </div>
        <div className="mt-10 flex justify-end gap-3">
          <VelionIconButton size="lg" radius="sm" aria-label={`${widget ? "Chat widget" : "Help page"} preview device`} className="border border-[#E2E3E8] dark:border-[#303238]">
            <Smartphone className="size-5" />
          </VelionIconButton>
          <VelionButton radius="sm" className="px-10 text-[13px] font-semibold">
            {widget ? "Manage" : "Setup"}
          </VelionButton>
        </div>
      </div>
    </article>
  );
}

export function ChannelCard({
  Icon,
  action,
  badge,
  description,
  title,
}: {
  Icon: LucideIcon;
  action: string;
  badge?: string;
  description: string;
  title: string;
}) {
  return (
    <article className="flex min-h-[260px] flex-col rounded-[12px] border border-[#E4E5E8] bg-white p-8 dark:border-[#303238] dark:bg-[#15161A]">
      <span className="grid size-16 place-items-center rounded-[10px] bg-[#F4F5F7] text-[#1D74F5] dark:bg-[#111216]">
        <Icon className="size-8" />
      </span>
      <h2 className="mt-7 flex items-center gap-3 text-[22px] font-semibold">
        {title}
        {badge ? <span className="rounded-full bg-[#111111] px-3 py-1 text-[12px] font-semibold text-white">{badge}</span> : null}
      </h2>
      <p className="mt-3 text-[17px] leading-7 text-[#555B65] dark:text-[#AEB4C0]">{description}</p>
      <div className="mt-auto flex justify-end gap-3">
        <VelionIconButton size="lg" radius="sm" aria-label={`${title} device preview`} className="border border-[#E2E3E8] dark:border-[#303238]">
          <Smartphone className="size-5" />
        </VelionIconButton>
        <VelionButton radius="sm" className="px-7 text-[13px] font-semibold">{action}</VelionButton>
      </div>
    </article>
  );
}

export function SquareIconButton({ Icon, label }: { Icon: LucideIcon; label: string }) {
  return (
    <VelionIconButton size="lg" radius="sm" aria-label={label} className="border border-[#E2E3E8] bg-white shadow-sm dark:border-[#303238] dark:bg-[#15161A]">
      <Icon className="size-5" />
    </VelionIconButton>
  );
}
