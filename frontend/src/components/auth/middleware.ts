import { NextRequest, NextResponse } from "next/server";
// Note: better-auth/cookies should only be used in middleware.ts file context
// For other contexts, use client-safe session checks
 
export async function middleware(request: NextRequest) {
    // Simple cookie check without importing server-only utilities
    // This is an optimistic check for performance - the actual session validation 
    // happens server-side in each protected page/route for security
    const sessionCookie = request.cookies.get('better-auth.session_token') || 
                         request.cookies.get('session_token') ||
                         request.cookies.get('auth-session');
 
    // If no session cookie exists, redirect to sign-in page
    if (!sessionCookie) {
        // Preserve the original URL as a redirect parameter
        const signInUrl = new URL("/sign-in", request.url);
        signInUrl.searchParams.set("redirect", request.nextUrl.pathname);
        return NextResponse.redirect(signInUrl);
    }
 
    // Allow the request to continue if session cookie exists
    // IMPORTANT: This only checks for cookie presence, not validity!
    // Each protected page MUST validate the session on the server side using:
    // - requireAuth() for pages that need authentication
    // - getServerSession() for conditional authentication
    return NextResponse.next();
}
 
export const config = {
  matcher: [
    // Protected routes that require authentication
    "/dashboard/:path*",
    "/profile/:path*",
    "/settings/:path*",
    
    // Add other protected routes here as needed
    // You can also use negative lookahead to exclude certain paths:
    // "/((?!api|_next/static|_next/image|favicon.ico|sign-in|sign-up).+)"
  ],
};