import { ContactsWorkspacePage } from '@/components/dashboard/product-section-pages';

export default function PeopleSectionPage({ params }: { params: { slug?: string[] } }) {
  return <ContactsWorkspacePage />;
}
