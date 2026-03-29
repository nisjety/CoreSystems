'use client';

import { useState } from 'react';
import { HelpCircle, Search } from 'lucide-react';
import { Button } from '@/components/ui/button';

const faqs = [
  {
    question: 'How do I get started?',
    answer: 'Getting started is easy. Create an organization, select your products, and configure your settings.',
  },
  {
    question: 'What are the different product options?',
    answer: 'We offer Quarry for analytics, Agenci for content management, and SEO tools for search optimization.',
  },
  {
    question: 'How do I invite team members?',
    answer: 'Go to your organization settings and use the members panel to invite teammates by email.',
  },
  {
    question: 'Is there a free plan?',
    answer: 'Yes, we offer a free plan to get started. Upgrade to Pro or Enterprise as you grow.',
  },
];

export default function AnswersPage() {
  const [searchQuery, setSearchQuery] = useState('');

  const filteredFaqs = faqs.filter(
    (faq) =>
      faq.question.toLowerCase().includes(searchQuery.toLowerCase()) ||
      faq.answer.toLowerCase().includes(searchQuery.toLowerCase()),
  );

  return (
    <div className="h-full overflow-y-auto bg-gradient-to-br from-slate-50 to-slate-100 p-8">
      <div className="max-w-2xl mx-auto">
        <div className="text-center mb-12">
          <div className="flex justify-center mb-4">
            <HelpCircle className="h-12 w-12 text-blue-600" />
          </div>
          <h1 className="text-4xl font-bold text-slate-900 mb-2">Answers & Help</h1>
          <p className="text-lg text-slate-600">Find answers to common questions</p>
        </div>

        {/* Search */}
        <div className="mb-8">
          <div className="relative">
            <Search className="absolute left-3 top-3 h-5 w-5 text-slate-400" />
            <input
              type="text"
              placeholder="Search answers..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className="w-full pl-10 pr-4 py-3 rounded-lg border border-slate-300 focus:border-blue-500 focus:outline-none focus:ring-2 focus:ring-blue-500/20"
            />
          </div>
        </div>

        {/* FAQs */}
        <div className="space-y-4">
          {filteredFaqs.length > 0 ? (
            filteredFaqs.map((faq, index) => (
              <div key={index} className="bg-white rounded-lg p-6 shadow-sm hover:shadow-md transition-shadow">
                <h3 className="font-semibold text-slate-900 mb-2">{faq.question}</h3>
                <p className="text-slate-600">{faq.answer}</p>
              </div>
            ))
          ) : (
            <div className="text-center py-12">
              <p className="text-slate-500">No matching answers found</p>
            </div>
          )}
        </div>

        {/* Contact Support */}
        <div className="mt-12 pt-8 border-t border-slate-200">
          <div className="text-center">
            <p className="text-slate-600 mb-4">Can't find what you're looking for?</p>
            <Button>Contact Support</Button>
          </div>
        </div>
      </div>
    </div>
  );
}
