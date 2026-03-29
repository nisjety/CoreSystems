'use client';

import Image from 'next/image';
import { LiquidCorner } from './LiquidCorner';

export interface DashboardCardData {
  id: string;
  category: string;
  title: string;
  description: string;
  image: string;
  href: string;
  prompt: string;
}

interface DashboardCardProps {
  card: DashboardCardData;
  stat?: string | null;
  onChatClick?: (prompt: string) => void;
}

export function DashboardCard({ card, stat, onChatClick }: DashboardCardProps) {
  return (
    <div className="group relative h-full overflow-hidden rounded-[18px] bg-white p-2.5 shadow-[0_2px_10px_rgba(0,0,0,0.05)]">
      <div className="pointer-events-none absolute left-0 top-0 z-30 rounded-br-[30px] bg-white px-5 pb-4 pt-5 text-[11px] font-semibold tracking-wide text-[#1A1A1A]">
        {card.category}
      </div>

      <a href={card.href} className="block">
        <div className="relative aspect-4/3 overflow-hidden rounded-[15px]">
          <Image
            src={card.image}
            alt={card.title}
            fill
            sizes="(max-width: 640px) 100vw, (max-width: 1024px) 70vw, 31vw"
            className="object-cover transition-transform duration-700 group-hover:scale-[1.02]"
          />

          <div className="absolute inset-x-0 bottom-0 z-10 h-3/5 bg-linear-to-t from-black/70 via-black/30 to-transparent" />

          <div className="pointer-events-none absolute inset-x-0 bottom-0 z-20 p-4 pb-16">
            {stat && (
              <p className="mb-1 text-[10px] font-semibold uppercase tracking-widest text-white/60">
                {stat}
              </p>
            )}
            <h3 className="text-[16px] font-semibold leading-snug text-white">
              {card.title}
            </h3>
            <p className="mt-1 line-clamp-2 text-[12px] leading-relaxed text-white/70">
              {card.description}
            </p>
          </div>
        </div>
      </a>

      <LiquidCorner 
        className="absolute -bottom-px right-2.5 z-40 h-[86px] w-40" 
        onClick={() => onChatClick?.(card.prompt)}
      />
    </div>
  );
}
