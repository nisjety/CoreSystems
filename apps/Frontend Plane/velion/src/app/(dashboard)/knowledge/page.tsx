import Link from 'next/link';
import {
  FileText,
  Type,
  Globe,
  MessageCircleQuestion,
  Plug,
  ArrowRight,
  Network,
  BookOpen,
} from 'lucide-react';
import { loadKnowledgeStats } from '@/components/knowledge/server/load-knowledge-stats';
import { KNOWLEDGE_FEATURE_FLAGS } from '@/lib/knowledge/feature-flags';
import type { KnowledgeMode } from '@/components/knowledge/types';

export const dynamic = 'force-dynamic';

/**
 * Wave 11 overview. Hub-style: each card deep-links into its sub-page.
 * Auth is enforced by the parent layout via `requireEdgeUser('/knowledge')`,
 * so this page can stay focused on presentation.
 *
 * Wave 11.C-a: Graph + Wiki cards are appended when their feature flags
 * are on. They render with a `Beta` tag and no count pill (the stats
 * loader doesn't aggregate graph/wiki counts yet).
 */
interface OverviewCard {
  href: string;
  label: string;
  description: string;
  icon: typeof FileText;
  modeKey?: KnowledgeMode;
  beta?: boolean;
}

const BASE_CARDS: ReadonlyArray<OverviewCard> = [
  {
    href: '/knowledge/files',
    label: 'Files',
    description: 'Upload PDFs, Word docs, Markdown, plain text, or CSVs.',
    icon: FileText,
    modeKey: 'files',
  },
  {
    href: '/knowledge/text',
    label: 'Text snippets',
    description: 'Paste short rules, brand voice samples, or quick facts.',
    icon: Type,
    modeKey: 'text',
  },
  {
    href: '/knowledge/website',
    label: 'Website',
    description: 'Crawl pages from a URL with Quarry. Preview before commit.',
    icon: Globe,
    modeKey: 'website',
  },
  {
    href: '/knowledge/qa',
    label: 'Q&A pairs',
    description: 'Operator-curated questions and answers, rated by use.',
    icon: MessageCircleQuestion,
    modeKey: 'qa',
  },
  {
    href: '/knowledge/integrations',
    label: 'Integrations',
    description: 'Sync from Google Drive, OneDrive, Notion, Slack, and more.',
    icon: Plug,
    modeKey: 'integrations',
  },
];

const GRAPH_CARD: OverviewCard = {
  href: '/knowledge/graph',
  label: 'Knowledge graph',
  description:
    'Entities and relationships extracted from your sources. Logseq-style force-directed view.',
  icon: Network,
  beta: true,
};

const WIKI_CARD: OverviewCard = {
  href: '/knowledge/wiki',
  label: 'LLM Wiki',
  description:
    'Durable, versioned wiki pages synthesized from sources. Edit in a block outline.',
  icon: BookOpen,
  beta: true,
};

export default async function KnowledgeOverviewPage() {
  const stats = await loadKnowledgeStats();
  const cards: ReadonlyArray<OverviewCard> = [
    ...BASE_CARDS,
    ...(KNOWLEDGE_FEATURE_FLAGS.graph ? [GRAPH_CARD] : []),
    ...(KNOWLEDGE_FEATURE_FLAGS.wiki ? [WIKI_CARD] : []),
  ];

  return (
    <div className="px-6 py-6">
      <header className="mb-6">
        <h1 className="text-[22px] font-semibold tracking-[-0.02em] text-[#111827]">
          Overview
        </h1>
        <p className="mt-1 max-w-[60ch] text-[13px] leading-6 text-[#6B7280]">
          Everything your agents can retrieve. Add sources, edit what they say, and
          retrain to pick up changes.
        </p>
      </header>

      <ul className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
        {cards.map((card) => {
          const Icon = card.icon;
          const count = card.modeKey ? stats.byMode[card.modeKey] : null;
          return (
            <li key={card.href}>
              <Link
                href={card.href}
                className="group block rounded-xl border border-[#E5E7EB] bg-white p-4 transition hover:border-[#111111] hover:shadow-[0_2px_10px_rgba(17,24,39,0.05)]"
              >
                <div className="flex items-start justify-between gap-3">
                  <span className="inline-flex size-8 items-center justify-center rounded-md bg-[#F3F4F6] text-[#111827]">
                    <Icon className="size-4" strokeWidth={1.8} />
                  </span>
                  <ArrowRight className="size-3.5 text-[#9CA3AF] transition group-hover:translate-x-0.5 group-hover:text-[#111827]" />
                </div>
                <h2 className="mt-3 flex items-center gap-2 text-[14px] font-semibold text-[#111827]">
                  <span>{card.label}</span>
                  {count !== null ? (
                    <span className="font-mono text-[12px] font-normal text-[#6B7280]">
                      · {count}
                    </span>
                  ) : null}
                  {card.beta ? (
                    <span className="rounded-sm bg-[#111111] px-1 py-0 text-[9px] font-semibold uppercase tracking-wide text-white">
                      Beta
                    </span>
                  ) : null}
                </h2>
                <p className="mt-1 text-[12px] leading-5 text-[#6B7280]">
                  {card.description}
                </p>
              </Link>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
