import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { LogoLoadingPreview } from "@/components/dev/logo-loading-preview/LogoLoadingPreview";

export const metadata: Metadata = {
	title: "Verevon · logo-intro",
	robots: { index: false, follow: false },
};

export default function LogoLoadingScreenPage() {
	if (process.env.NODE_ENV !== "development") notFound();
	return <LogoLoadingPreview />;
}
