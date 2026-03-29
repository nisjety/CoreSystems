import { NextRequest, NextResponse } from 'next/server';

const SESSION_COOKIES = [
  'idknuten.sid',
  'better-auth.session_token',
  'session_token',
];

const PROTECTED_PREFIXES = [
  '/dashboard',
  // '/planner', // TODO: re-enable auth when testing is done
  '/profile',
  '/settings',
  '/workspace',
  '/onboarding',
];

// NOTE: /sign-in is intentionally NOT in this list so that users with stale
// session cookies (session expired server-side but cookie still in browser)
// can still reach the sign-in page. AuthPage handles the redirect to /dashboard
// client-side when the user actually has a valid session.
const AUTH_ONLY_PATHS = ['/sign-up'];

function hasSessionCookie(request: NextRequest): boolean {
  if (SESSION_COOKIES.some((name) => !!request.cookies.get(name))) {
    return true;
  }

  return request.cookies
    .getAll()
    .some(
      ({ name }) =>
        name === 'sid' ||
        name.endsWith('.sid') ||
        name.includes('.sid.') ||
        name.includes('session_token') ||
        name.includes('better-auth.session_token'),
    );
}

export function proxy(request: NextRequest) {
  const { pathname } = request.nextUrl;
  const isAuthenticated = hasSessionCookie(request);

  if (
    process.env.NODE_ENV === 'development' &&
    (pathname.startsWith('/dashboard') || pathname.startsWith('/sign-in'))
  ) {
    const cookieNames = request.cookies.getAll().map((cookie) => cookie.name);
    console.log(
      `[proxy] ${pathname} auth=${isAuthenticated} cookies=${cookieNames.join(',')}`,
    );
  }

  if (isAuthenticated && AUTH_ONLY_PATHS.some((path) => pathname.startsWith(path))) {
    return NextResponse.redirect(new URL('/dashboard', request.url));
  }

  if (PROTECTED_PREFIXES.some((prefix) => pathname.startsWith(prefix))) {
    if (!isAuthenticated) {
      const signInUrl = new URL('/sign-in', request.url);
      signInUrl.searchParams.set('redirect', pathname);
      return NextResponse.redirect(signInUrl);
    }
  }

  return NextResponse.next();
}

export const config = {
  matcher: [
    '/dashboard',
    '/dashboard/:path*',
    '/profile',
    '/profile/:path*',
    '/settings',
    '/settings/:path*',
    '/workspace',
    '/workspace/:path*',
    '/onboarding',
    '/onboarding/:path*',
    '/sign-up',
  ],
};
