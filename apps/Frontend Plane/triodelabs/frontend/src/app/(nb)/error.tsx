"use client";

import { ErrorView } from "@/components/feedback/status-pages";

export default function Error({
  error,
  reset,
}: {
  error: Error & { digest?: string };
  reset: () => void;
}) {
  return <ErrorView error={error} locale="nb" reset={reset} />;
}
