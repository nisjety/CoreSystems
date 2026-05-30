'use client';

import React from 'react';
import { m } from 'framer-motion';
import { DashboardCard, DashboardCardData } from './DashboardCard';

const EMPTY_STATS: Record<string, string | null> = {};

interface DashboardCardsProps {
  cards: DashboardCardData[];
  stats?: Record<string, string | null>;
  onChatClick?: (prompt: string) => void;
}

export function DashboardCards({ cards, stats = EMPTY_STATS, onChatClick }: DashboardCardsProps) {
  const scrollerRef = React.useRef<HTMLDivElement | null>(null);
  const pageRefs = React.useRef<Array<HTMLDivElement | null>>([]);
  const [activePage, setActivePage] = React.useState(0);
  const [viewportHeight, setViewportHeight] = React.useState<number | null>(null);
  const [isAtEnd, setIsAtEnd] = React.useState(false);

  const pages = React.useMemo(() => {
    const grouped: DashboardCardData[][] = [];

    cards.forEach((card, index) => {
      const pageIndex = Math.floor(index / 3);

      if (!grouped[pageIndex]) {
        grouped[pageIndex] = [];
      }

      grouped[pageIndex].push(card);
    });

    return grouped;
  }, [cards]);

  React.useEffect(() => {
    const measure = () => {
      const firstPage = pageRefs.current[0];

      if (firstPage) {
        setViewportHeight(firstPage.offsetHeight);
      }
    };

    let frameId: number | null = null;
    let observer: ResizeObserver | null = null;

    const attachObserver = () => {
      const firstPage = pageRefs.current[0];

      if (!firstPage || typeof ResizeObserver === 'undefined') {
        return;
      }

      observer = new ResizeObserver(measure);
      observer.observe(firstPage);
    };

    measure();
    frameId = window.requestAnimationFrame(() => {
      measure();
      attachObserver();
    });

    window.addEventListener('resize', measure);

    return () => {
      if (frameId !== null) {
        cancelAnimationFrame(frameId);
      }
      window.removeEventListener('resize', measure);
      observer?.disconnect();
    };
  }, [pages]);

  const scrollToPage = (index: number) => {
    const scroller = scrollerRef.current;
    const page = pageRefs.current[index];

    if (!scroller || !page) {
      return;
    }

    scroller.scrollTo({
      top: page.offsetTop,
      behavior: 'smooth',
    });

    setActivePage(index);
  };

  const handleAdvance = () => {
    const scroller = scrollerRef.current;

    if (pages.length <= 1 || !scroller || !viewportHeight) {
      return;
    }

    if (isAtEnd) {
      scroller.scrollBy({
        top: -viewportHeight,
        behavior: 'smooth',
      });
      return;
    }

    scroller.scrollBy({
      top: viewportHeight,
      behavior: 'smooth',
    });
  };

  const handleScroll = () => {
    const scroller = scrollerRef.current;

    if (!scroller || !viewportHeight) {
      return;
    }

    const nextPage = Math.round(scroller.scrollTop / viewportHeight);
    const boundedPage = Math.max(0, Math.min(pages.length - 1, nextPage));
    const atEnd = scroller.scrollTop + scroller.clientHeight >= scroller.scrollHeight - 4;

    setIsAtEnd(atEnd);

    if (boundedPage !== activePage) {
      setActivePage(boundedPage);
    }
  };

  if (cards.length === 0) {
    return (
      <p className="py-16 text-center text-[13px] text-[#C8C1B3]">
        Ingen resultater funnet.
      </p>
    );
  }

  return (
    <div className="space-y-4">
      <div className="overflow-hidden" style={viewportHeight ? { height: `${viewportHeight}px` } : undefined}>
        <div
          ref={scrollerRef}
          onScroll={handleScroll}
          className="max-h-full snap-y snap-mandatory overflow-y-auto scroll-smooth [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
        >
          <div className="flex flex-col gap-4">
            {pages.map((page, pageIndex) => (
              <div
                key={`page-${pageIndex}`}
                ref={(node) => {
                  pageRefs.current[pageIndex] = node;
                }}
                className="snap-start"
              >
                <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
                  {page.map((card, cardIndex) => (
                    <m.div
                      key={card.id}
                      initial={{ opacity: 0, y: 28 }}
                      whileInView={{ opacity: 1, y: 0 }}
                      transition={{
                        duration: 0.55,
                        ease: [0.25, 0.46, 0.45, 0.94],
                        delay: cardIndex * 0.09,
                      }}
                      viewport={{ root: scrollerRef, once: true, margin: '-5%' }}
                    >
                      <DashboardCard
                        card={card}
                        stat={stats[card.id]}
                        onChatClick={onChatClick}
                      />
                    </m.div>
                  ))}
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>

      {pages.length > 1 && (
        <div className="flex justify-center">
          <button
            type="button"
            onClick={handleAdvance}
            className="jump-down flex h-12 w-12 items-center justify-center transition-transform duration-300 hover:scale-105"
            aria-label={isAtEnd ? 'Rull opp til forrige kortside' : 'Rull ned til neste kortside'}
          >
            <span
              className="icon-arrow-down relative block h-5 w-8 transition-transform duration-300"
              style={{ transform: isAtEnd ? 'rotate(180deg)' : 'rotate(0deg)' }}
              aria-hidden="true"
            >
              <span
                className="absolute -left-1 top-1/2 h-[3px] w-[25px] -translate-y-1/2 rotate-45 rounded-full bg-[#171D18]"
              />
              <span
                className="absolute -right-1 top-2/5 h-[3px] w-[22px] -translate-y-1/2 -rotate-45 rounded-full bg-[#171D18]"
              />
            </span>
          </button>
        </div>
      )}
    </div>
  );
}
