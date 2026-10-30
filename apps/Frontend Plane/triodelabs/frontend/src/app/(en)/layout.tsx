import type { Metadata } from "next";
import "@/styles/globals.css";
import { getRootMetadata, RootDocument } from "@/components/layout/root-document";

export const metadata: Metadata = getRootMetadata("en");

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return <RootDocument locale="en">{children}</RootDocument>;
}
