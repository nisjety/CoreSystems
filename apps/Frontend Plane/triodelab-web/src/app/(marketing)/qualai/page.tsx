import type { Metadata } from 'next'

export const metadata: Metadata = {
  title: 'QualAI – Web Management Platform | Triodelab',
  description: 'QualAI er Triodelabs plattform for lenkevalidering, WCAG-tilsyn og LIKS-lesbarhetsanalyse. AI-støttet analyse for offentlige nettsider – kommer snart.',
}

export default function QualaiPage() {
  return (
    <div className="flex min-h-screen items-center justify-center bg-[#F4F1EB]">
      <div className="text-center">
        <h1 
          className="mb-4 text-6xl font-light tracking-wide text-[#111111]"
          style={{ fontFamily: 'var(--font-cormorant-garamond), serif' }}
        >
          Qualai
        </h1>
        <p className="text-[#6A655F]" style={{ fontFamily: 'Inter, sans-serif' }}>
          Web Management Platform
        </p>
        <p className="mt-8 text-sm text-[#6A655F]" style={{ fontFamily: 'Inter, sans-serif' }}>
          Coming Soon
        </p>
      </div>
    </div>
  );
}
