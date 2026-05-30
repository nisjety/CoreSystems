"use client";

import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { AudioWaveform, Mic, X } from "lucide-react";
import { cn } from "@/lib/utils";

export function RealtimeVoiceModal({
  language,
  model,
  onClose,
  open,
}: {
  language: string;
  model: string;
  onClose: () => void;
  open: boolean;
}) {
  const [listening, setListening] = useState(false);
  const streamRef = useRef<MediaStream | null>(null);

  useEffect(
    () => () => {
      streamRef.current?.getTracks().forEach((track) => track.stop());
    },
    [],
  );

  if (!open || typeof document === "undefined") {
    return null;
  }

  const toggleListening = async () => {
    if (listening) {
      streamRef.current?.getTracks().forEach((track) => track.stop());
      streamRef.current = null;
      setListening(false);
      return;
    }

    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      streamRef.current = stream;
      setListening(true);
    } catch {
      setListening(false);
    }
  };

  return createPortal(
    <dialog
      open
      className="fixed inset-0 z-[10000] m-0 grid h-auto max-h-none w-auto max-w-none place-items-center border-0 bg-black/20 px-4 text-inherit backdrop-blur-sm"
      aria-label="Voice mode"
      data-dashboard-modal="true"
    >
      <button
        type="button"
        aria-label="Dismiss voice backdrop"
        className="absolute inset-0 cursor-default"
        onClick={onClose}
      />
      <div className="velion-panel-in relative z-10 w-full max-w-sm rounded-[24px] bg-white p-4 shadow-[0_24px_80px_rgba(0,0,0,0.18)] dark:bg-[#141516]">
        <div className="flex items-start justify-between gap-4">
          <div className="flex items-center gap-3">
            <span className="grid size-10 place-items-center rounded-2xl bg-black/[0.04] text-[#1a1a1a] dark:bg-white/10 dark:text-white">
              <AudioWaveform className="size-5" />
            </span>
            <div>
              <p className="text-[14px] font-semibold text-[#1a1a1a] dark:text-white">Voice mode</p>
              <p className="text-[12px] text-[#888]">{model} · {language}</p>
            </div>
          </div>
          <button type="button" onClick={onClose} className="rounded-xl p-2 text-[#888] transition-colors hover:bg-black/5 dark:hover:bg-white/10" aria-label="Close voice mode">
            <X className="size-4" />
          </button>
        </div>

        <div className="mt-5 rounded-[18px] bg-[#f7f7f8] p-4 text-center dark:bg-white/10">
          <div className={cn("mx-auto grid size-16 place-items-center rounded-full", listening ? "bg-orange-50 text-orange-500" : "bg-white text-[#666] dark:bg-[#141516] dark:text-[#D3D7DE]")}>
            <Mic className="size-6" />
          </div>
          <p className="mt-3 text-[13px] font-medium text-[#1a1a1a] dark:text-white">
            {listening ? "Listening…" : "Ready for a live voice session"}
          </p>
          <p className="mt-1 text-[12px] leading-5 text-[#888]">
            Realtime endpoint pending.
          </p>
        </div>

        <button
          type="button"
          onClick={() => void toggleListening()}
          className="mt-4 h-10 w-full rounded-[14px] bg-[#1a1a1a] text-[13px] font-semibold text-white transition-colors hover:bg-[#333]"
        >
          {listening ? "Stop listening" : "Start listening"}
        </button>
      </div>
    </dialog>,
    document.body,
  );
}
