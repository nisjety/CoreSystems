import { Suspense } from 'react';
import { redirect } from 'next/navigation';
import { ChatPage } from '@/components/chat/components/ChatPage';

type ChatRouteProps = {
  searchParams?: Promise<{ message?: string; q?: string; new?: string }>;
};

export default async function ChatRoute({ searchParams }: ChatRouteProps) {
  const resolvedSearchParams = searchParams ? await searchParams : undefined;
  if (resolvedSearchParams?.message || resolvedSearchParams?.q || resolvedSearchParams?.new) {
    redirect('/chat');
  }

  return (
    <Suspense fallback={<div className="flex h-full min-h-0 items-center justify-center bg-[var(--linear-main-bg)]"><div className="h-6 w-6 animate-spin rounded-full border border-[var(--linear-border)] border-t-[#26282f]" /></div>}>
      <ChatPage key="chat-page-new" />
    </Suspense>
  );
}
