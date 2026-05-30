import { Suspense } from 'react';
import { redirect } from 'next/navigation';
import { ChatPage } from '@/components/chat/components/ChatPage';

type ChatSessionPageProps = {
  params: Promise<{ id: string }>;
  searchParams?: Promise<{ message?: string; q?: string; new?: string }>;
};

export default async function ChatSessionPage({ params, searchParams }: ChatSessionPageProps) {
  const resolvedParams = await params;
  const resolvedSearchParams = searchParams ? await searchParams : undefined;

  if (resolvedSearchParams?.message || resolvedSearchParams?.q || resolvedSearchParams?.new) {
    redirect(`/chat/${resolvedParams.id}`);
  }

  return (
    <Suspense fallback={<div className="flex h-full min-h-0 items-center justify-center bg-[var(--linear-main-bg)]"><div className="h-6 w-6 animate-spin rounded-full border border-[var(--linear-border)] border-t-[#26282f]" /></div>}>
      <ChatPage key={`chat-page-${resolvedParams.id}`} routeSessionId={resolvedParams.id} />
    </Suspense>
  );
}
