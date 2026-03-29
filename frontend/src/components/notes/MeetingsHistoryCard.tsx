'use client';

import { Avatar, AvatarFallback } from '@/components/ui/avatar';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Filter } from 'lucide-react';

interface Meeting {
  id: string;
  title: string;
  timeframe: string;
  date: string;
  actionCount: number;
  tags: string[];
  participants: string[];
  duration: string;
}

interface MeetingsHistoryCardProps {
  meetings: ReadonlyArray<Meeting>;
  activeMeetingId: Meeting['id'];
  onMeetingSelect: (id: Meeting['id']) => void;
}

export function MeetingsHistoryCard({
  meetings,
  activeMeetingId,
  onMeetingSelect,
}: MeetingsHistoryCardProps) {
  return (
    <aside className="w-[20rem] shrink-0">
      <div className="flex h-[80vh] flex-col items-start gap-2 rounded-lg bg-white p-8 shadow-md">
        <div className="flex w-full items-center justify-between">
          <h2 className="text-lg font-semibold text-slate-900">Meetings History</h2>
          <Button
            variant="outline"
            className="inline-flex items-center gap-1 rounded-full border-slate-300 bg-white px-4 py-1.5 text-xs font-semibold text-slate-700 shadow-sm hover:bg-white"
          >
            <Filter className="h-3.5 w-3.5" />
            Filter
          </Button>
        </div>
        <div className="mt-5 flex-1 w-full space-y-3 overflow-y-auto pr-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
          {meetings.map((meeting) => {
            const isActive = meeting.id === activeMeetingId;
            return (
              <button
                key={meeting.id}
                onClick={() => onMeetingSelect(meeting.id)}
                className={`group w-full rounded-3xl px-4 py-4 text-left shadow-sm transition-all duration-150 hover:-translate-y-px ${
                  isActive
                    ? 'bg-slate-900/5 shadow-lg ring-2 ring-slate-900/10'
                    : 'bg-slate-100 hover:bg-white hover:shadow-md'
                }`}
              >
                <h3 className="line-clamp-2 text-sm font-semibold text-slate-900">{meeting.title}</h3>
                <div className="mt-2 flex items-center gap-2 text-xs font-medium text-slate-500">
                  <span className="text-slate-600">{meeting.timeframe}</span>
                  <span className="text-slate-400">•</span>
                  <span className="text-slate-400">{meeting.date}</span>
                </div>
                <div className="mt-3 flex items-center gap-2">
                  <span className="inline-flex h-6 min-w-[26px] items-center justify-center rounded-full border border-slate-300 bg-white px-2 text-[11px] font-semibold text-slate-900">
                    {meeting.actionCount}
                  </span>
                  {meeting.tags.map((tag) => (
                    <Badge
                      key={`${meeting.id}-${tag}`}
                      variant="outline"
                      className="rounded-full border-slate-300 bg-white px-3 py-0.5 text-[11px] font-medium text-slate-700"
                    >
                      {tag}
                    </Badge>
                  ))}
                </div>
                <div className="mt-3 flex -space-x-2">
                  {meeting.participants.map((initials) => (
                    <Avatar
                      key={`${meeting.id}-${initials}`}
                      className="h-8 w-8 border-2 border-white bg-slate-200 text-[11px] font-semibold text-slate-700 shadow-sm"
                    >
                      <AvatarFallback>{initials}</AvatarFallback>
                    </Avatar>
                  ))}
                </div>
              </button>
            );
          })}
        </div>
      </div>
    </aside>
  );
}
