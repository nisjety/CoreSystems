'use client';

import Link from 'next/link';
import { CircleDashed } from 'lucide-react';
import { SimpleTooltip } from '../ui/simple-tooltip';

export function MinimizedLogo() {
  return (
    <SimpleTooltip content="Home" placement="right" delay={200}>
      <Link
        href="/dashboard"
        className="flex h-7 w-7 items-center justify-center rounded-lg transition-colors hover:bg-black/5 focus:outline-none focus-visible:ring-2 focus-visible:ring-[#DD7A1F]/40 focus-visible:ring-offset-1"
        title="Go to home"
        aria-label="Go to home"
      >
        <CircleDashed className="h-4 w-4 text-[#DD7A1F]" strokeWidth={1.8} />
      </Link>
    </SimpleTooltip>
  );
}
