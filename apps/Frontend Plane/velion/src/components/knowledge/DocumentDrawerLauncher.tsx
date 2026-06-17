'use client';

import { useRouter } from 'next/navigation';
import { type ReactElement } from 'react';
import { DocumentDrawer } from './DocumentDrawer';

interface DocumentDrawerLauncherProps {
  documentId: string;
}

/**
 * Wave 11 §5 — the page-level launcher that wraps the drawer in a
 * route shell. The "main" content behind the drawer is intentionally
 * empty — closing the drawer routes back to the parent list page.
 */
export function DocumentDrawerLauncher({
  documentId,
}: DocumentDrawerLauncherProps): ReactElement {
  const router = useRouter();

  const handleClose = (): void => {
    router.back();
  };

  return (
    <>
      <div className="flex h-full items-center justify-center px-6 text-[#9CA3AF]">
        <p className="text-[12px]">Document detail open. Close to return.</p>
      </div>
      <DocumentDrawer
        documentId={documentId}
        onClose={handleClose}
        onDeleted={() => router.push('/knowledge/files')}
      />
    </>
  );
}
