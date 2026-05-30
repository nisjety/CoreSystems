function AgentCardSkeleton() {
  return (
    <div className="flex h-full flex-col rounded-[20px] border border-[#E5E7EE] bg-white p-5">
      <div className="flex items-start gap-3">
        <div className="size-10 shrink-0 animate-pulse rounded-[10px] bg-[#F0F1F5]" />
        <div className="min-w-0 flex-1 space-y-2 pt-0.5">
          <div className="h-4 w-3/4 animate-pulse rounded-md bg-[#F0F1F5]" />
          <div className="h-3 w-1/3 animate-pulse rounded-md bg-[#F0F1F5]" />
        </div>
      </div>
      <div className="mt-4 flex-1 space-y-2">
        <div className="h-3 w-full animate-pulse rounded-md bg-[#F0F1F5]" />
        <div className="h-3 w-4/5 animate-pulse rounded-md bg-[#F0F1F5]" />
      </div>
      <div className="mt-4 space-y-1 border-t border-[#F0F1F5] pt-3">
        <div className="h-3 w-1/3 animate-pulse rounded-md bg-[#F0F1F5]" />
        <div className="h-3 w-1/4 animate-pulse rounded-md bg-[#F0F1F5]" />
      </div>
    </div>
  );
}

export function AgentsSkeleton() {
  return (
    <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
      <AgentCardSkeleton />
      <AgentCardSkeleton />
      <AgentCardSkeleton />
    </div>
  );
}
