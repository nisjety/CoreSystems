"use client";

import Image from "next/image";
import { useEffect, useRef } from "react";
import {
	Check,
	ChevronRight,
	Clock3,
	FileText,
	History,
	Search,
	ShieldCheck,
	Sparkles,
} from "lucide-react";

export type ProductLoopMedia = {
	alt: string;
	kind: "approval" | "image" | "knowledge" | "video";
	objectPosition?: string;
	poster?: string;
	src?: string;
};

function ProductLoopVideo({ media }: { media: ProductLoopMedia }) {
	const videoRef = useRef<HTMLVideoElement>(null);
	useEffect(() => {
		const video = videoRef.current;
		if (!video) return;
		const preference = window.matchMedia("(prefers-reduced-motion: reduce)");
		const layer = video.closest<HTMLElement>("[data-product-loop-layer]");
		let isVisible = false;
		const update = () => {
			const layerVisible = !layer || (getComputedStyle(layer).visibility !== "hidden" && Number(getComputedStyle(layer).opacity) > 0.1);
			if (isVisible && layerVisible && !preference.matches && !document.hidden) {
				void video.play().catch(() => {});
			} else video.pause();
		};
		const observer = new IntersectionObserver(([entry]) => { isVisible = entry.isIntersecting; update(); });
		const layerObserver = new MutationObserver(update);
		observer.observe(video);
		if (layer) layerObserver.observe(layer, { attributes: true, attributeFilter: ["style"] });
		preference.addEventListener("change", update);
		document.addEventListener("visibilitychange", update);
		return () => { observer.disconnect(); layerObserver.disconnect(); preference.removeEventListener("change", update); document.removeEventListener("visibilitychange", update); video.pause(); };
	}, []);
	return <video ref={videoRef} aria-label={media.alt} className="h-full w-full object-contain" muted playsInline poster={media.poster} preload="none" style={{ objectPosition: media.objectPosition ?? "center" }}><source src={media.src} type="video/mp4" /></video>;
}

export function ProductLoopMediaContent({
	media,
	priority = false,
}: {
	media: ProductLoopMedia;
	priority?: boolean;
}) {
	if (media.kind === "approval") {
		return <ApprovalWorkspaceDemo />;
	}

	if (media.kind === "knowledge") {
		return <KnowledgeRevisionDemo />;
	}

	if (media.kind === "video" && media.src) {
		return <ProductLoopVideo media={media} />;
	}

	if (!media.src) {
		return null;
	}

	return (
		<Image
			alt={media.alt}
			className="object-cover saturate-[0.88] contrast-[1.02]"
			fill
			loading={priority ? "eager" : "lazy"}
			sizes="(max-width: 899px) 92vw, 72vw"
			src={media.src}
			style={{ objectPosition: media.objectPosition ?? "center" }}
		/>
	);
}

function Logo({ label, src }: { label: string; src: string }) {
	return (
		<span
			className="grid size-[clamp(18px,2.1vw,30px)] shrink-0 place-items-center rounded-[7px] border border-black/8 bg-white shadow-[0_3px_10px_rgba(22,22,20,0.06)]"
			title={label}
		>
			<Image alt="" height={17} src={src} width={17} />
		</span>
	);
}

function DemoShell({
	children,
	section,
}: {
	children: React.ReactNode;
	section: string;
}) {
	return (
		<div className="h-full w-full overflow-hidden bg-[#f7f7f5] font-protokoll text-[#1d1d1b]">
			<header className="flex h-[9%] min-h-8 items-center gap-[2%] border-b border-black/8 bg-white/86 px-[2.2%] backdrop-blur-xl">
				<span className="grid aspect-square h-[54%] min-h-5 place-items-center rounded-[7px] border border-black/9 bg-[#1d1d1b] font-arbeit text-[clamp(9px,1vw,14px)] text-white">
					V
				</span>
				<span className="text-[clamp(8px,0.85vw,12px)] text-black/40">
					Verevon AS
				</span>
				<ChevronRight className="size-[clamp(8px,0.85vw,12px)] text-black/25" />
				<span className="text-[clamp(8px,0.85vw,12px)] font-medium">
					{section}
				</span>
				<div className="ml-auto flex h-[58%] w-[29%] items-center gap-2 rounded-full border border-black/7 bg-[#fafaf9] px-[3%] text-black/32">
					<Search className="size-[clamp(8px,0.9vw,13px)]" />
					<span className="truncate text-[clamp(7px,0.72vw,11px)]">
						Søk i kunnskapsbasen
					</span>
				</div>
			</header>
			<div className="grid h-[91%] grid-cols-[8%_92%]">
				<aside className="flex flex-col items-center gap-[5%] border-r border-black/7 bg-white/52 py-[14%]">
					{[0, 1, 2, 3, 4, 5].map((item) => (
						<span
							className={`block aspect-square w-[33%] rounded-[4px] ${
								item === 2 ? "bg-[#ef7d59]" : "border border-black/12"
							}`}
							key={item}
						/>
					))}
				</aside>
				{children}
			</div>
		</div>
	);
}

export function ApprovalWorkspaceDemo() {
	return (
		<DemoShell section="Innboks">
			<div className="grid min-w-0 grid-cols-[25%_43%_32%]">
				<section className="min-w-0 border-r border-black/8 bg-white/36 p-[7%]">
					<p className="text-[clamp(8px,0.9vw,13px)] font-medium">Innboks</p>
					<div className="mt-[7%] space-y-[4%]">
						{[
							["Mina", "Hvor er pakken min?", true],
							["Anders", "Endre leveringsadresse", false],
							["Sara", "Retur av bestilling", false],
						].map(([name, subject, active]) => (
							<div
								className={`rounded-[9px] border p-[6%] ${
									active
										? "border-[#ef7d59]/28 bg-white shadow-[0_7px_22px_rgba(26,26,23,0.07)]"
										: "border-transparent bg-transparent"
								}`}
								key={String(name)}
							>
								<p className="truncate text-[clamp(7px,0.78vw,11px)] font-medium">
									{name}
								</p>
								<p className="mt-1 truncate text-[clamp(6px,0.68vw,10px)] text-black/43">
									{subject}
								</p>
							</div>
						))}
					</div>
				</section>

				<section className="min-w-0 p-[5%]">
					<div className="flex items-center gap-2 border-b border-black/7 pb-[4%]">
						<div className="grid size-[clamp(17px,2vw,30px)] place-items-center rounded-full bg-[#e7ddff] text-[clamp(7px,0.8vw,11px)] font-medium">
							M
						</div>
						<div>
							<p className="text-[clamp(7px,0.82vw,12px)] font-medium">Mina Solberg</p>
							<p className="text-[clamp(6px,0.62vw,9px)] text-black/38">E-post · VIP-kunde</p>
						</div>
					</div>
					<div className="mt-[6%] rounded-[10px] bg-white p-[5%] text-[clamp(7px,0.76vw,11px)] leading-[1.45] shadow-[0_6px_20px_rgba(25,25,22,0.05)]">
						Pakken har stått på Oslo-terminalen siden fredag. Kan dere hjelpe?
					</div>
					<div className="ml-[9%] mt-[5%] rounded-[10px] border border-[#ef7d59]/16 bg-[#fff8f4] p-[5%]">
						<div className="mb-[3%] flex items-center gap-2 text-[clamp(6px,0.66vw,10px)] font-medium text-[#ad5036]">
							<Sparkles className="size-[clamp(8px,0.9vw,13px)]" /> Verevon-utkast
						</div>
						<p className="text-[clamp(7px,0.74vw,11px)] leading-[1.45]">
							Jeg har sjekket sporingen. Pakken er forsinket, og vi refunderer frakten.
						</p>
						<div className="mt-[5%] flex items-center gap-[3%]">
							<Logo label="Bring" src="/brand-logos/bring.svg" />
							<Logo label="Notion" src="/brand-logos/notion.svg" />
							<span className="text-[clamp(6px,0.6vw,9px)] text-black/38">2 kilder kontrollert</span>
						</div>
					</div>
				</section>

				<aside className="min-w-0 border-l border-black/8 bg-white/65 p-[7%]">
					<div className="flex items-center gap-2 text-[clamp(7px,0.78vw,11px)] font-medium text-[#9c4b35]">
						<ShieldCheck className="size-[clamp(10px,1.1vw,16px)]" /> Krever godkjenning
					</div>
					<h3 className="mt-[8%] font-arbeit text-[clamp(14px,1.75vw,27px)] font-light leading-[1.02] tracking-[-0.04em]">
						Refunder frakt
					</h3>
					<p className="mt-[3%] text-[clamp(7px,0.77vw,11px)] text-black/48">149 kr · policy 04</p>
					<div className="mt-[9%] space-y-[5%] border-y border-black/7 py-[7%] text-[clamp(6px,0.68vw,10px)] text-black/54">
						<p className="flex items-center gap-2"><Check className="size-[clamp(8px,0.85vw,12px)] text-emerald-600" /> Beløp innenfor grense</p>
						<p className="flex items-center gap-2"><Clock3 className="size-[clamp(8px,0.85vw,12px)]" /> Sporingen er 3 dager forsinket</p>
					</div>
					<div className="mt-[9%] grid grid-cols-2 gap-[5%]">
						<button className="rounded-full border border-black/12 bg-white py-[8%] text-[clamp(6px,0.67vw,10px)]">Revider</button>
						<button className="rounded-full bg-[#1d1d1b] py-[8%] text-[clamp(6px,0.67vw,10px)] text-white">Godkjenn</button>
					</div>
				</aside>
			</div>
		</DemoShell>
	);
}

export function KnowledgeRevisionDemo() {
	return (
		<DemoShell section="Kunnskap">
			<div className="grid min-w-0 grid-cols-[26%_46%_28%]">
				<section className="min-w-0 border-r border-black/8 bg-white/38 p-[7%]">
					<p className="text-[clamp(8px,0.9vw,13px)] font-medium">Kilder</p>
					<div className="mt-[8%] space-y-[4%]">
						{[
							["Retur og refusjon", "/brand-logos/notion.svg", "Notion"],
							["Kundehenvendelser", "/brand-logos/outlook.svg", "Outlook"],
							["Driftsrutiner", "/brand-logos/sharepoint.svg", "SharePoint"],
						].map(([name, src, label], index) => (
							<div className={`flex items-center gap-[5%] rounded-[9px] border p-[5%] ${index === 0 ? "border-[#ef7d59]/25 bg-white" : "border-transparent"}`} key={name}>
								<Logo label={label} src={src} />
								<span className="truncate text-[clamp(6px,0.7vw,10px)]">{name}</span>
							</div>
						))}
					</div>
				</section>

				<article className="min-w-0 p-[6%]">
					<div className="flex items-center gap-2 text-[clamp(6px,0.66vw,10px)] text-black/38">
						<FileText className="size-[clamp(9px,0.95vw,14px)]" /> Policy · oppdatert nå
					</div>
					<h3 className="mt-[5%] font-arbeit text-[clamp(16px,2.2vw,34px)] font-light tracking-[-0.045em]">Forsinket levering</h3>
					<p className="mt-[7%] text-[clamp(7px,0.77vw,12px)] leading-[1.55] text-black/58">
						Når en sending er forsinket mer enn to virkedager, kan kunden få frakten refundert.
					</p>
					<div className="mt-[6%] rounded-[9px] border border-[#ef7d59]/24 bg-[#fff6f1] p-[5%] text-[clamp(7px,0.74vw,11px)] leading-[1.48]">
						<span className="mb-2 block text-[clamp(6px,0.62vw,9px)] font-medium uppercase tracking-[0.14em] text-[#a84931]">Ny presisering</span>
						Verevon skal alltid vise sporingskilden før refusjonen godkjennes.
					</div>
					<div className="mt-[8%] flex items-center gap-[3%] text-[clamp(6px,0.64vw,10px)] text-black/39">
						<History className="size-[clamp(9px,0.95vw,14px)]" /> Versjon 12 · endret av Local
					</div>
				</article>

				<aside className="min-w-0 border-l border-black/8 bg-white/62 p-[7%]">
					<div className="grid size-[clamp(22px,2.7vw,42px)] place-items-center rounded-full bg-emerald-50 text-emerald-700">
						<Check className="size-[48%]" />
					</div>
					<h3 className="mt-[8%] font-arbeit text-[clamp(13px,1.65vw,25px)] font-light leading-[1.08] tracking-[-0.035em]">Neste svar er allerede bedre.</h3>
					<p className="mt-[6%] text-[clamp(6px,0.7vw,10px)] leading-[1.5] text-black/48">
						Regelen er oppdatert for 4 agenter. Endringen er sporbar og kan rulles tilbake.
					</p>
					<div className="mt-[10%] rounded-[9px] border border-black/8 bg-white p-[6%]">
						<p className="text-[clamp(6px,0.62vw,9px)] text-black/36">Synkronisert med</p>
						<div className="mt-[5%] flex gap-[5%]">
							<Logo label="Notion" src="/brand-logos/notion.svg" />
							<Logo label="Outlook" src="/brand-logos/outlook.svg" />
							<Logo label="Microsoft Teams" src="/brand-logos/teams.svg" />
						</div>
					</div>
				</aside>
			</div>
		</DemoShell>
	);
}
