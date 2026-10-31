import { renderOgImage } from "@/components/seo/og-image";

export const dynamic = "force-static";

export function GET() {
  return renderOgImage("en");
}
