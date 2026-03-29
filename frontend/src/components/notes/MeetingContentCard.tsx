'use client';

import { Fragment } from 'react';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { Mic, Share2, SquareStack, Users, Video } from 'lucide-react';

interface ActionItem {
  text: string;
  timestamp: string;
}

interface MeetingContentCardProps {
  title: string;
  duration: string;
  actionItems: ActionItem[];
}

const renderWithApostrophes = (text: string) =>
  text.split("'").map((segment, index, array) => (
    <Fragment key={`${segment}-${index}`}>
      {segment}
      {index < array.length - 1 && <>&apos;</>}
    </Fragment>
  ));

export function MeetingContentCard({ title, duration, actionItems }: MeetingContentCardProps) {
  return (
    <section className="flex w-120 flex-col items-start gap-2">
      <Card className="flex h-[80vh] w-full flex-col items-start gap-2 overflow-hidden rounded-3xl border-0 bg-white p-8 shadow-md">
        <div className="flex items-center justify-between">
          <div>
            <p className="text-xs font-medium uppercase tracking-[0.18em] text-slate-500">Meetings History</p>
            <h2 className="mt-1 text-base font-semibold text-slate-900">{title}</h2>
          </div>
        </div>
        <div className="relative mt-3 flex h-64 w-104 overflow-hidden rounded-3xl bg-linear-to-br from-slate-900 via-slate-800 to-slate-900 p-8">
          <div className="grid grid-cols-2 gap-3 p-4">
            {[1, 2].map((slot) => (
              <div
                key={`participant-${slot}`}
                className="flex min-h-[120px] flex-col justify-end rounded-2xl border border-white/10 bg-linear-to-br from-slate-700/80 to-slate-600/60 p-3 text-white"
              >
                <div className="flex items-center gap-3">
                  <div className="flex h-12 w-12 items-center justify-center rounded-full border border-white/30 bg-white/10">
                    <Users className="h-6 w-6" />
                  </div>
                  <div>
                    <p className="text-sm font-medium">Speaker {slot}</p>
                    <p className="text-xs text-white/70">Connected</p>
                  </div>
                </div>
              </div>
            ))}
          </div>
          <div className="absolute left-6 bottom-6 inline-flex items-center gap-2 rounded-full bg-black/60 px-4 py-1 text-sm font-medium text-white">
            <Mic className="h-4 w-4" />
            {duration}
          </div>
          <div className="absolute inset-x-0 bottom-0 flex justify-center pb-4">
            <div className="flex items-center gap-4 rounded-full border border-white/10 bg-white/10 px-4 py-2 text-white/80 backdrop-blur">
              <Mic className="h-4 w-4" />
              <Video className="h-4 w-4" />
              <SquareStack className="h-4 w-4" />
              <Share2 className="h-4 w-4" />
            </div>
          </div>
        </div>

        <Card className="mt-3 flex-1 overflow-y-auto rounded-3xl border-0 bg-slate-50 shadow-none [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
          <CardContent className="space-y-3 p-4">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-3">
                <span className="inline-flex h-10 w-10 items-center justify-center rounded-full bg-slate-900 text-base font-bold text-white">
                  12
                </span>
                <p className="text-base font-semibold text-slate-900">Action List</p>
              </div>
              <Button className="rounded-full bg-slate-900 px-6 py-2.5 text-sm font-semibold text-white shadow hover:bg-slate-800">
                Create Project
              </Button>
            </div>
            <div className="space-y-2">
              {actionItems.map((item) => (
                <div key={item.text} className="flex items-start gap-2.5 rounded-2xl bg-white px-3 py-2 text-xs text-slate-800">
                  <span className="mt-0.5 inline-flex h-4 w-4 items-center justify-center rounded-full border-2 border-slate-500"></span>
                  <div className="flex-1">
                    <p className="leading-relaxed">
                      {renderWithApostrophes(item.text)}
                      {item.timestamp && <span className="ml-1 font-medium text-blue-600">{item.timestamp}</span>}
                    </p>
                  </div>
                </div>
              ))}
            </div>
          </CardContent>
        </Card>
      </Card>
    </section>
  );
}
