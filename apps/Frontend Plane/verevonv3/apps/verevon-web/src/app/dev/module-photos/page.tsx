import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { ModulePhotoReview } from "./photo-review";

export const metadata: Metadata = {
	title: "Bildevalg til modulene",
	robots: { index: false, follow: false },
};

export default function ModulePhotosPage() {
	if (process.env.NODE_ENV !== "development") notFound();
	return <ModulePhotoReview />;
}
