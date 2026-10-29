import type { ReactNode } from "react";

export function ServicesSectionLabel({ children }: { children: ReactNode }) {
  return (
    <p className="font-label text-[0.68rem] uppercase tracking-[0.16em] text-[#8C8C87]">
      {children}
    </p>
  );
}
