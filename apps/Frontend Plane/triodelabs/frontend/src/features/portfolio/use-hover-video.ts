"use client";

import {
  useEffect,
  useRef,
  useState,
  type FocusEvent,
  type PointerEvent,
} from "react";

export function useHoverVideo(enabled: boolean) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const playbackRequested = useRef(false);
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  const [visible, setVisible] = useState(true);
  const [playing, setPlaying] = useState(false);

  useEffect(() => {
    const updateVisibility = () =>
      setVisible(document.visibilityState === "visible");
    updateVisibility();
    document.addEventListener("visibilitychange", updateVisibility);
    return () =>
      document.removeEventListener("visibilitychange", updateVisibility);
  }, []);

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    let cancelled = false;
    const stop = () => {
      video.pause();
      // An unloaded video has no seekable timeline yet.
      if (video.readyState > 0) video.currentTime = 0;
      setPlaying(false);
    };

    playbackRequested.current = enabled && visible && (hovered || focused);
    if (playbackRequested.current) {
      video
        .play()
        .then(() => {
          if (cancelled) {
            if (!playbackRequested.current) video.pause();
          } else {
            setPlaying(true);
          }
        })
        .catch(() => {
          // Autoplay policies and decoding/network failures retain the poster.
          if (!cancelled) stop();
        });
    } else {
      stop();
    }

    return () => {
      cancelled = true;
      playbackRequested.current = false;
      stop();
    };
  }, [enabled, visible, hovered, focused]);

  return {
    videoRef,
    playing,
    onVideoError: () => {
      videoRef.current?.pause();
      setPlaying(false);
    },
    handlers: {
      onPointerEnter: (event: PointerEvent<HTMLAnchorElement>) => {
        if (event.pointerType === "mouse") setHovered(true);
      },
      onPointerLeave: () => setHovered(false),
      onPointerDown: () => setFocused(false),
      onFocus: (event: FocusEvent<HTMLAnchorElement>) => {
        setFocused(event.currentTarget.matches(":focus-visible"));
      },
      onBlur: () => setFocused(false),
    },
  };
}
