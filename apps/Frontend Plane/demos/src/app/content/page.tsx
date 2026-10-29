'use client';

import { useState } from 'react';
import { FileText, Plus, Search, Edit2, Trash2, Eye } from 'lucide-react';
import { Button } from '@/components/ui/button';

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
    <div className="min-h-screen bg-gradient-to-br from-slate-50 to-slate-100 p-8">
      <div className="max-w-6xl mx-auto">
        {/* Header */}
        <div className="flex justify-between items-start mb-8">
          <div>
            <h1 className="text-4xl font-bold text-slate-900 flex items-center gap-3 mb-2">
              <FileText className="h-10 w-10" />
              Content Management
            </h1>
            <p className="text-slate-600">Create and manage your content</p>
          </div>
          <Button>
            <Plus className="h-4 w-4 mr-2" />
            New Content
          </Button>
        </div>

        {/* Search */}
        <div className="mb-6">
          <div className="relative">
            <Search className="absolute left-3 top-3 h-5 w-5 text-slate-400" />
            <input
              type="text"
              placeholder="Search content..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className="w-full pl-10 pr-4 py-3 rounded-lg border border-slate-300 focus:border-blue-500 focus:outline-none focus:ring-2 focus:ring-blue-500/20"
            />
          </div>
        </div>

        {/* Content Table */}
        <div className="bg-white rounded-lg shadow-sm overflow-hidden">
          <table className="w-full">
            <thead className="bg-slate-50 border-b border-slate-200">
              <tr>
                <th className="px-6 py-3 text-left text-sm font-semibold text-slate-900">Title</th>
                <th className="px-6 py-3 text-left text-sm font-semibold text-slate-900">Type</th>
                <th className="px-6 py-3 text-left text-sm font-semibold text-slate-900">Status</th>
                <th className="px-6 py-3 text-left text-sm font-semibold text-slate-900">Updated</th>
                <th className="px-6 py-3 text-left text-sm font-semibold text-slate-900">Actions</th>
              </tr>
            </thead>
            <tbody>
              {filteredContent.length > 0 ? (
                filteredContent.map((item) => (
                  <tr key={item.id} className="border-b border-slate-200 hover:bg-slate-50">
                    <td className="px-6 py-4 text-slate-900 font-medium">{item.title}</td>
                    <td className="px-6 py-4 text-slate-600 capitalize">{item.type}</td>
                    <td className="px-6 py-4">
                      <span
                        className={`inline-block px-3 py-1 rounded-full text-sm font-medium ${
                          item.status === 'published'
                            ? 'bg-green-100 text-green-800'
                            : 'bg-yellow-100 text-yellow-800'
                        }`}
                      >
                        {item.status}
                      </span>
                    </td>
                    <td className="px-6 py-4 text-slate-600">{item.updated}</td>
                    <td className="px-6 py-4 flex gap-2">
                      <button className="p-2 hover:bg-slate-200 rounded-lg transition-colors" title="View">
                        <Eye className="h-4 w-4 text-slate-600" />
                      </button>
                      <button className="p-2 hover:bg-slate-200 rounded-lg transition-colors" title="Edit">
                        <Edit2 className="h-4 w-4 text-slate-600" />
                      </button>
                      <button
                        onClick={() => handleDelete(item.id)}
                        className="p-2 hover:bg-red-100 rounded-lg transition-colors"
                        title="Delete"
                      >
                        <Trash2 className="h-4 w-4 text-red-600" />
                      </button>
                    </td>
                  </tr>
                ))
              ) : (
                <tr>
                  <td colSpan={5} className="px-6 py-12 text-center text-slate-500">
                    No content found
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
