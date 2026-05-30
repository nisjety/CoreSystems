"use client";

import { useSyncExternalStore } from "react";

export function useClientTodayKey() {
  const todayKey = useSyncExternalStore(subscribeToToday, getTodaySnapshot, getServerTodaySnapshot);
  return todayKey || null;
}

function subscribeToToday(onStoreChange: () => void) {
  if (typeof window === "undefined") {
    return () => {};
  }

  let timeout = 0;

  const scheduleNextMidnight = () => {
    const now = new Date();
    const nextMidnight = new Date(now);
    nextMidnight.setHours(24, 0, 0, 0);

    timeout = window.setTimeout(() => {
      onStoreChange();
      scheduleNextMidnight();
    }, Math.max(1_000, nextMidnight.getTime() - now.getTime()));
  };

  scheduleNextMidnight();
  return () => window.clearTimeout(timeout);
}

function getTodaySnapshot() {
  if (typeof window === "undefined") {
    return "";
  }

  return formatDateKey(new Date());
}

function getServerTodaySnapshot() {
  return "";
}

function formatDateKey(date: Date) {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}
