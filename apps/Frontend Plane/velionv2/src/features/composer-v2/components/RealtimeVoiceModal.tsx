"use client";

import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { AudioWaveform, Mic, X } from "lucide-react";
import { cn } from "@/lib/utils";

// Minimal Web Speech API surface (not in lib.dom types). Client-side STT — no
// backend needed; the realtime gateway endpoint can replace this later.
type SpeechResultLike = { readonly 0: { transcript: string }; isFinal: boolean };
type SpeechEventLike = { resultIndex: number; results: ArrayLike<SpeechResultLike> };
type SpeechRecognitionLike = {
  lang: string;
  continuous: boolean;
  interimResults: boolean;
  onresult: ((event: SpeechEventLike) => void) | null;
  onend: (() => void) | null;
  onerror: (() => void) | null;
  start: () => void;
  stop: () => void;
};
type SpeechRecognitionCtor = new () => SpeechRecognitionLike;

function getSpeechRecognitionCtor(): SpeechRecognitionCtor | null {
  if (typeof window === "undefined") {
    return null;
  }
  const w = window as unknown as {
    SpeechRecognition?: SpeechRecognitionCtor;
    webkitSpeechRecognition?: SpeechRecognitionCtor;
  };
  return w.SpeechRecognition ?? w.webkitSpeechRecognition ?? null;
}

export function RealtimeVoiceModal({
  language,
  model,
  onClose,
  onTranscript,
}: {
  language: string;
  model: string;
  onClose: () => void;
  onTranscript?: (text: string) => void;
}) {
  const [listening, setListening] = useState(false);
  const [transcript, setTranscript] = useState("");
  const [interim, setInterim] = useState("");
  const recognitionRef = useRef<SpeechRecognitionLike | null>(null);
  const supported = getSpeechRecognitionCtor() !== null;

  const stop = () => {
    recognitionRef.current?.stop();
    recognitionRef.current = null;
    setListening(false);
    setInterim("");
  };

  useEffect(() => {
    return () => {
      recognitionRef.current?.stop();
      recognitionRef.current = null;
    };
  }, []);

  if (typeof document === "undefined") {
    return null;
  }

  const startListening = () => {
    const Ctor = getSpeechRecognitionCtor();
    if (!Ctor) {
      return;
    }
    const recognition = new Ctor();
    recognition.lang = language || "nb-NO";
    recognition.continuous = true;
    recognition.interimResults = true;
    recognition.onresult = (event) => {
      let finalChunk = "";
      let interimChunk = "";
      for (let i = event.resultIndex; i < event.results.length; i += 1) {
        const result = event.results[i];
        const text = result[0]?.transcript ?? "";
        if (result.isFinal) {
          finalChunk += text;
        } else {
          interimChunk += text;
        }
      }
      if (finalChunk) {
        setTranscript((current) => (current ? `${current} ${finalChunk.trim()}` : finalChunk.trim()));
      }
      setInterim(interimChunk);
    };
    recognition.onend = () => setListening(false);
    recognition.onerror = () => setListening(false);
    recognitionRef.current = recognition;
    setListening(true);
    recognition.start();
  };

  const toggleListening = () => {
    if (listening) {
      stop();
    } else {
      startListening();
    }
  };

  const handleClose = () => {
    stop();
    setTranscript("");
    onClose();
  };

  const insert = () => {
    const text = `${transcript} ${interim}`.trim();
    if (text && onTranscript) {
      onTranscript(text);
    }
    stop();
    setTranscript("");
    onClose();
  };

  const liveText = `${transcript}${interim ? ` ${interim}` : ""}`.trim();

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
        onClick={handleClose}
      />
      <div className="verevon-panel-in relative z-10 w-full max-w-sm rounded-[24px] bg-white p-4 shadow-[0_24px_80px_rgba(0,0,0,0.18)] dark:bg-[#141516]">
        <div className="flex items-start justify-between gap-4">
          <div className="flex items-center gap-3">
            <span className="grid size-10 place-items-center rounded-2xl bg-black/[0.04] text-[#1a1a1a] dark:bg-white/10 dark:text-white">
              <AudioWaveform className="size-5" />
            </span>
            <div>
              <p className="text-[14px] font-semibold text-[#1a1a1a] dark:text-white">Stemmemodus</p>
              <p className="text-[12px] text-[#888]">
                {model} · {language}
              </p>
            </div>
          </div>
          <button
            type="button"
            onClick={handleClose}
            className="rounded-xl p-2 text-[#888] transition-colors hover:bg-black/5 dark:hover:bg-white/10"
            aria-label="Lukk stemmemodus"
          >
            <X className="size-4" />
          </button>
        </div>

        <div className="mt-5 rounded-[18px] bg-[#f7f7f8] p-4 text-center dark:bg-white/10">
          <div
            className={cn(
              "mx-auto grid size-16 place-items-center rounded-full transition-colors",
              listening
                ? "verevon-voice-pulse bg-[#FBF0E4] text-[#C07B33]"
                : "bg-white text-[#666] dark:bg-[#141516] dark:text-[#D3D7DE]",
            )}
          >
            <Mic className="size-6" />
          </div>
          {liveText ? (
            <p className="mt-3 max-h-28 overflow-y-auto whitespace-pre-wrap text-left text-[13px] leading-[1.5] text-[#1a1a1a] dark:text-white">
              {transcript}
              {interim ? <span className="text-[#9AA0A9]"> {interim}</span> : null}
            </p>
          ) : (
            <>
              <p className="mt-3 text-[13px] font-medium text-[#1a1a1a] dark:text-white">
                {listening ? "Lytter …" : supported ? "Klar for diktering" : "Stemme støttes ikke i denne nettleseren"}
              </p>
              <p className="mt-1 text-[12px] leading-5 text-[#888]">
                {supported
                  ? "Snakk fritt — teksten settes inn i meldingen."
                  : "Prøv Chrome/Edge, eller skriv meldingen i stedet."}
              </p>
            </>
          )}
        </div>

        <div className="mt-4 flex gap-2">
          <button
            type="button"
            onClick={toggleListening}
            disabled={!supported}
            className="h-10 flex-1 rounded-[14px] bg-[#1a1a1a] text-[13px] font-semibold text-white transition-colors hover:bg-[#333] disabled:opacity-40 dark:bg-white dark:text-[#111111]"
          >
            {listening ? "Stopp" : "Start diktering"}
          </button>
          <button
            type="button"
            onClick={insert}
            disabled={!liveText}
            className="h-10 flex-1 rounded-[14px] border border-[#E2E3E9] bg-white text-[13px] font-semibold text-[#1a1a1a] transition-colors hover:bg-black/5 disabled:opacity-40 dark:border-[#2A2C31] dark:bg-[#17181C] dark:text-white dark:hover:bg-white/10"
          >
            Sett inn
          </button>
        </div>
      </div>
    </dialog>,
    document.body,
  );
}
