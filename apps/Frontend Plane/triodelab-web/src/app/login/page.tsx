import { redirect } from 'next/navigation';

export default function LoginRedirectPage() {
  redirect(`${process.env.NEXT_PUBLIC_AGENCIA_URL || 'http://localhost:3000'}/login`);
}
