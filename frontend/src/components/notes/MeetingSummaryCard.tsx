'use client';

import { Fragment } from 'react';
import { Button } from '@/components/ui/button';
import { Separator } from '@/components/ui/separator';
import { Download, Share2 } from 'lucide-react';

interface DiscussionPoint {
  text: string;
  references: string[];
}

interface Objection {
  text: string;
  references: string[];
}

interface MeetingSummaryCardProps {
  discussionPoints: DiscussionPoint[];
  objections: Objection[];
  actionItems: string[];
}

const renderWithApostrophes = (text: string) =>
  text.split("'").map((segment, index, array) => (
    <Fragment key={`${segment}-${index}`}>
      {segment}
      {index < array.length - 1 && <>&apos;</>}
    </Fragment>
  ));

export function MeetingSummaryCard({
  discussionPoints,
  objections,
  actionItems,
}: MeetingSummaryCardProps) {
  return (
    <div className="flex h-full flex-col overflow-hidden">
      <div className="flex-1 overflow-y-auto px-4 py-4 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
        <div className="space-y-4">
          <div>
            <h3 className="text-base font-semibold text-slate-900">Meeting Summary</h3>
          </div>
          <div className="space-y-3">
            <div>
              <h4 className="text-sm font-semibold text-slate-900">Discussion points</h4>
              <div className="mt-3 space-y-3">
                {discussionPoints.map((point) => (
                  <div key={point.text} className="space-y-1 text-sm text-slate-800">
                    <p className="leading-relaxed">• {renderWithApostrophes(point.text)}</p>
                    <div className="flex gap-3 pl-5 text-xs font-medium text-blue-600">
                      {point.references.map((ref) => (
                        <span key={`${point.text}-${ref}`}>{ref}</span>
                      ))}
                    </div>
                  </div>
                ))}
              </div>
            </div>
            <Separator />
            <div>
              <h4 className="text-sm font-semibold text-slate-900">Objections</h4>
              <div className="mt-3 space-y-3">
                {objections.map((item) => (
                  <div key={item.text} className="space-y-1 text-sm text-slate-800">
                    <p className="leading-relaxed">• {renderWithApostrophes(item.text)}</p>
                    <div className="flex gap-3 pl-5 text-xs font-medium text-blue-600">
                      {item.references.map((ref) => (
                        <span key={`${item.text}-${ref}`}>{ref}</span>
                      ))}
                    </div>
                  </div>
                ))}
              </div>
            </div>
            <Separator />
            <div>
              <h4 className="text-sm font-semibold text-slate-900">Action items</h4>
              <div className="mt-3 space-y-2 text-sm text-slate-800">
                {actionItems.map((item) => (
                  <p key={item}>• {renderWithApostrophes(item)}</p>
                ))}
              </div>
            </div>
          </div>
        </div>
      </div>
      <div className="flex items-center justify-between px-4 pb-3 pt-2">
        <Button className="rounded-full bg-slate-900 px-5 py-2 text-sm font-semibold text-white shadow hover:bg-slate-800">
          Create Project
        </Button>
        <div className="flex items-center gap-2">
          <Button variant="ghost" size="icon" className="h-9 w-9 rounded-full">
            <Download className="h-4 w-4 text-slate-600" />
          </Button>
          <Button variant="ghost" size="icon" className="h-9 w-9 rounded-full">
            <Share2 className="h-4 w-4 text-slate-600" />
          </Button>
        </div>
      </div>
    </div>
  );
}
