import type { Metadata } from "next";
import "@/styles/globals.css";
import { GlobalNotFoundDocument } from "@/components/feedback/global-not-found-document";

// Rendered for URLs that match no route. There is no shared root layout
// (each language group has its own), so this page renders its own document.
export const metadata: Metadata = {
  title: "404 | TriodeLab",
  robots: { index: false },
};

export default function GlobalNotFound() {
  return <GlobalNotFoundDocument />;
}
