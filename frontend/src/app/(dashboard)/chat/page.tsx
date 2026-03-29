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
    <Suspense fallback={<div className="flex h-full min-h-0 items-center justify-center bg-[#F4F1EB]"><div className="h-6 w-6 animate-spin rounded-full border border-[#D8D2C6] border-t-[#2B2B2B]" /></div>}>
      <ChatPage key="chat-page-new" />
    </Suspense>
  );
}
