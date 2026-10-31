import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { LogoMotionStudies } from "@/components/dev/logo-motion-studies/LogoMotionStudies";

export const metadata: Metadata = {
	title: "Verevon · merkebevegelse",
	robots: { index: false, follow: false },
};

export default function LogoMotionStudiesPage() {
	if (process.env.NODE_ENV !== "development") notFound();
	return <LogoMotionStudies />;
}
