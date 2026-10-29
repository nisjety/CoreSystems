import type { Metadata } from 'next';

export const metadata: Metadata = {
  title: 'Avelis Overview',
  description: 'Avelis provides the website optimization workspace within the CoreSystem ecosystem.',
};

export default function HomePage() {
  return (
    <main>
      <h1>Avelis</h1>
      <p>Frontend shell for website optimization workflows.</p>
    </main>
  );
}