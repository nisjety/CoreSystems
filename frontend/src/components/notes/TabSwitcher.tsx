'use client';

import { TabsList, TabsTrigger } from '@/components/ui/tabs';
import { NotebookPen, SquareStack, StickyNote } from 'lucide-react';

export function TabSwitcher() {
  return (
    <div className="flex h-[10vh] items-end border-b border-slate-200 px-4 pb-3">
      <div className="flex items-center gap-6">
        <TabsList className="flex h-auto gap-0 bg-transparent p-0">
          <TabsTrigger
            value="summary"
            className="mr-6 flex items-center gap-2 rounded-none border-0 bg-transparent px-0 py-2 text-sm font-medium text-slate-500 data-[state=active]:border-b-2 data-[state=active]:border-slate-900 data-[state=active]:text-slate-900 data-[state=active]:shadow-none"
          >
            <StickyNote className="h-4 w-4" />
            Summary
          </TabsTrigger>
          <TabsTrigger
            value="transcript"
            className="mr-6 flex items-center gap-2 rounded-none border-0 bg-transparent px-0 py-2 text-sm font-medium text-slate-500 data-[state=active]:border-b-2 data-[state=active]:border-slate-900 data-[state=active]:text-slate-900 data-[state=active]:shadow-none"
          >
            <NotebookPen className="h-4 w-4" />
            Transcript
          </TabsTrigger>
          <TabsTrigger
            value="resource"
            className="flex items-center gap-2 rounded-none border-0 bg-transparent px-0 py-2 text-sm font-medium text-slate-500 data-[state=active]:border-b-2 data-[state=active]:border-slate-900 data-[state=active]:text-slate-900 data-[state=active]:shadow-none"
          >
            <SquareStack className="h-4 w-4" />
            Resource
          </TabsTrigger>
        </TabsList>
      </div>
    </div>
  );
}
