'use client';

import { useState } from 'react';
import { Search } from 'lucide-react';

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
    <div className="flex h-full min-h-0 flex-col overflow-y-auto bg-white">
      <div className="mx-auto w-full max-w-[580px] px-6 py-10">
        <h1 className="mb-10 text-[22px] font-semibold tracking-tight text-[#111111]">
          Answers &amp; Help
        </h1>

        <div className="relative mb-8">
          <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-[#BBBBBB]" />
          <input
            type="text"
            placeholder="Search answers..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="h-10 w-full rounded-[8px] border border-[#E0E0E0] pl-9 pr-3 text-[13px] text-[#111111] outline-none transition placeholder:text-[#BBBBBB] focus:border-[#999] focus:ring-2 focus:ring-black/5"
          />
        </div>

        <div className="border-t border-[#F0F0F0]">
          {filteredFaqs.length > 0 ? (
            filteredFaqs.map((faq) => (
              <div key={faq.question} className="border-b border-[#F0F0F0] py-4">
                <p className="text-[13px] font-medium text-[#111111]">{faq.question}</p>
                <p className="mt-0.5 text-[12px] leading-5 text-[#6B7280]">{faq.answer}</p>
              </div>
            ))
          ) : (
            <div className="py-12 text-center text-[13px] text-[#6B7280]">
              No matching answers found
            </div>
          )}
        </div>

        <div className="mt-8 border-t border-[#F0F0F0] pt-8">
          <p className="text-[13px] text-[#6B7280]">Can&apos;t find what you&apos;re looking for?</p>
          <button
            type="button"
            className="mt-4 inline-flex items-center gap-2 rounded-full bg-[#111111] px-4 py-2 text-[13px] font-medium text-white transition hover:bg-[#2B2B2B]"
          >
            Contact Support
          </button>
        </div>
      </div>
    </div>
  );
}
