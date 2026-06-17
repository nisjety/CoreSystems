'use client';

import { useState } from 'react';
import { Edit2, Eye, Plus, Search, Trash2 } from 'lucide-react';

interface ContentItem {
  id: string;
  title: string;
  type: 'page' | 'blog' | 'article';
  status: 'draft' | 'published';
  updated: string;
}

const mockContent: ContentItem[] = [
  { id: '1', title: 'Getting Started Guide', type: 'page', status: 'published', updated: '2 days ago' },
  { id: '2', title: 'Product Features Overview', type: 'article', status: 'published', updated: '1 week ago' },
  { id: '3', title: 'API Documentation', type: 'page', status: 'draft', updated: '3 hours ago' },
];

export default function ContentPage() {
  const [searchQuery, setSearchQuery] = useState('');
  const [content, setContent] = useState(mockContent);

  const filteredContent = content.filter((item) =>
    item.title.toLowerCase().includes(searchQuery.toLowerCase()),
  );

  const handleDelete = (id: string) => {
    setContent(content.filter((item) => item.id !== id));
  };

  return (
    <div className="flex h-full min-h-0 flex-col overflow-y-auto bg-white">
      <div className="mx-auto w-full max-w-[760px] px-6 py-10">
        <div className="mb-10 flex items-center justify-between">
          <h1 className="text-[22px] font-semibold tracking-tight text-[#111111]">Content</h1>
          <button
            type="button"
            className="inline-flex items-center gap-1.5 rounded-full bg-[#111111] px-4 py-2 text-[13px] font-medium text-white transition hover:bg-[#2B2B2B]"
          >
            <Plus className="h-3.5 w-3.5" />
            New
          </button>
        </div>

        <div className="relative mb-6">
          <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-[#BBBBBB]" />
          <input
            type="text"
            placeholder="Search content..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="h-10 w-full rounded-[8px] border border-[#E0E0E0] pl-9 pr-3 text-[13px] text-[#111111] outline-none transition placeholder:text-[#BBBBBB] focus:border-[#999] focus:ring-2 focus:ring-black/5"
          />
        </div>

        <div className="border-t border-[#F0F0F0]">
          <div className="grid grid-cols-[minmax(0,1fr)_80px_90px_100px_88px] gap-4 border-b border-[#F0F0F0] py-2">
            {['Title', 'Type', 'Status', 'Updated', ''].map((col) => (
              <span key={col} className="text-[11px] font-medium uppercase tracking-wide text-[#9BA3AF]">
                {col}
              </span>
            ))}
          </div>

          {filteredContent.length > 0 ? (
            filteredContent.map((item) => (
              <div
                key={item.id}
                className="grid grid-cols-[minmax(0,1fr)_80px_90px_100px_88px] items-center gap-4 border-b border-[#F0F0F0] py-3.5"
              >
                <span className="truncate text-[13px] font-medium text-[#111111]">{item.title}</span>
                <span className="text-[12px] capitalize text-[#6B7280]">{item.type}</span>
                <span
                  className={`inline-flex w-fit items-center rounded-full px-2 py-0.5 text-[11px] font-medium ${
                    item.status === 'published'
                      ? 'bg-[#DCFCE7] text-[#166534]'
                      : 'bg-[#FEF9C3] text-[#854D0E]'
                  }`}
                >
                  {item.status}
                </span>
                <span className="text-[12px] text-[#9BA3AF]">{item.updated}</span>
                <div className="flex gap-1">
                  <button
                    type="button"
                    title="View"
                    className="rounded-[6px] p-1.5 text-[#9BA3AF] transition-colors hover:bg-[#F4F6FA] hover:text-[#111111]"
                  >
                    <Eye className="h-3.5 w-3.5" />
                  </button>
                  <button
                    type="button"
                    title="Edit"
                    className="rounded-[6px] p-1.5 text-[#9BA3AF] transition-colors hover:bg-[#F4F6FA] hover:text-[#111111]"
                  >
                    <Edit2 className="h-3.5 w-3.5" />
                  </button>
                  <button
                    type="button"
                    title="Delete"
                    onClick={() => handleDelete(item.id)}
                    className="rounded-[6px] p-1.5 text-[#9BA3AF] transition-colors hover:bg-[#FEF2F2] hover:text-[#DC2626]"
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                  </button>
                </div>
              </div>
            ))
          ) : (
            <div className="py-12 text-center text-[13px] text-[#6B7280]">No content found</div>
          )}
        </div>
      </div>
    </div>
  );
}
