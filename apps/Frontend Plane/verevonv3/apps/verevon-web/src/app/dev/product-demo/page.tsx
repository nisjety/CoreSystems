import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { ProductDemo } from "./product-demo";

export const metadata: Metadata = { title: "Verevon · produktdemo", robots: { index: false, follow: false } };
export default function Page() {
  if (process.env.NODE_ENV !== "development") notFound();
  return <ProductDemo />;
}
