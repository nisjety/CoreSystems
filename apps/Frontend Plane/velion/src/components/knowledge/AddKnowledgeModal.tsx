'use client';

import { type ReactElement } from 'react';
import { useRouter } from 'next/navigation';
import {
  X,
  FileText,
  Type,
  Globe,
  MessageCircleQuestion,
  HardDrive,
  Cloud,
  BookOpen,
  Hash,
} from 'lucide-react';

interface AddKnowledgeModalProps {
  open: boolean;
  onClose: () => void;
}

interface Tile {
  key: string;
  label: string;
  description: string;
  icon: typeof FileText;
  /** Path to navigate to OR null to mark as "coming soon". */
  href: string | null;
  /** Optional: kick an OAuth flow instead of a route change. */
  oauthProvider?: 'google-drive' | 'microsoft' | 'notion' | 'slack';
}

const TILES: ReadonlyArray<Tile> = [
  {
    key: 'files',
    label: 'Files',
    description: 'PDF, DOCX, MD, TXT, CSV',
    icon: FileText,
    href: '/knowledge/files',
  },
  {
    key: 'text',
    label: 'Text',
    description: 'Paste a snippet',
    icon: Type,
    href: '/knowledge/text',
  },
  {
    key: 'website',
    label: 'Website',
    description: 'Crawl pages from a URL',
    icon: Globe,
    href: '/knowledge/website',
  },
  {
    key: 'qa',
    label: 'Q&A',
    description: 'Operator-curated pairs',
    icon: MessageCircleQuestion,
    href: '/knowledge/qa',
  },
  {
    key: 'gdrive',
    label: 'Google Drive',
    description: 'Connect via OAuth',
    icon: HardDrive,
    href: '/settings/integrations?connect=google-drive',
    oauthProvider: 'google-drive',
  },
  {
    key: 'onedrive',
    label: 'OneDrive',
    description: 'Connect via OAuth',
    icon: Cloud,
    href: '/settings/integrations?connect=onedrive',
    oauthProvider: 'microsoft',
  },
  {
    key: 'notion',
    label: 'Notion',
    description: 'Sync a workspace',
    icon: BookOpen,
    href: '/settings/integrations?connect=notion',
    oauthProvider: 'notion',
  },
  {
    key: 'slack',
    label: 'Slack',
    description: 'Sync channels',
    icon: Hash,
    href: '/settings/integrations?connect=slack',
    oauthProvider: 'slack',
  },
];

/**
 * Wave 11 §2.2 — Lindy 8-tile source picker (verified against Mobbin
 * screen `04b7331b-e1b4-4723-951d-3aac7e62a981`).
 *
 * Always-on tiles for inputs we own (files / text / website / Q&A); the
 * four OAuth tiles deep-link into the integrations sub-page which kicks
 * the existing integration-core OAuth flow. Tiles never lead to a dead
 * end — every click either routes to a working page or starts a real
 * OAuth dance.
 */
export function AddKnowledgeModal({ open, onClose }: AddKnowledgeModalProps): ReactElement | null {
  const router = useRouter();

  if (!open) return null;

  const handleTileClick = (tile: Tile): void => {
    if (!tile.href) return;
    router.push(tile.href);
    onClose();
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      role="dialog"
      aria-modal="true"
      aria-labelledby="add-knowledge-title"
      onClick={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div className="w-full max-w-[560px] rounded-2xl bg-white p-6 shadow-2xl">
        <div className="flex items-start justify-between gap-4">
          <div>
            <h2
              id="add-knowledge-title"
              className="text-[16px] font-semibold tracking-[-0.01em] text-[#111827]"
            >
              Add knowledge
            </h2>
            <p className="mt-0.5 text-[12px] text-[#6B7280]">
              Pick a source. You can add more anytime.
            </p>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label="Close"
            className="rounded-md p-1 text-[#6B7280] hover:bg-[#F3F4F6] hover:text-[#111827]"
          >
            <X className="size-4" />
          </button>
        </div>

        <ul className="mt-5 grid grid-cols-3 gap-3">
          {TILES.map((tile) => {
            const Icon = tile.icon;
            const enabled = Boolean(tile.href);
            return (
              <li key={tile.key}>
                <button
                  type="button"
                  onClick={() => handleTileClick(tile)}
                  disabled={!enabled}
                  className={`flex w-full flex-col items-start gap-2 rounded-xl border p-3 text-left transition ${
                    enabled
                      ? 'border-[#E5E7EB] bg-white hover:border-[#111111] hover:shadow-[0_2px_8px_rgba(17,24,39,0.06)]'
                      : 'cursor-not-allowed border-[#E5E7EB] bg-[#F9FAFB] opacity-60'
                  }`}
                >
                  <span className="inline-flex size-8 items-center justify-center rounded-md bg-[#F3F4F6] text-[#111827]">
                    <Icon className="size-4" strokeWidth={1.8} />
                  </span>
                  <span className="block text-[12px] font-semibold text-[#111827]">
                    {tile.label}
                  </span>
                  <span className="block text-[10px] leading-4 text-[#6B7280]">
                    {tile.description}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      </div>
    </div>
  );
}
