"use client";

import Image from "next/image";
import { Check, Search } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { ArrowButton } from "@/components/ui/ArrowButton";
import {
	INTEGRATION_CATALOG,
	INTEGRATION_CATEGORIES,
	type Integration,
	type IntegrationAvailability,
	type IntegrationCategory,
} from "./integration-catalog";

const categoryLabels: Record<IntegrationCategory, string> = {
	commerce: "Handel og logistikk",
	collaboration: "Samarbeid",
	customer: "Kunde og marked",
	microsoft: "Microsoft",
	"public-data": "Offentlige data",
	finance: "Økonomi og drift",
	developer: "Utvikling og automasjon",
	knowledge: "Dokumenter og sak",
};

const availabilityLabels: Record<IntegrationAvailability, string> = {
	available: "Kobles i dag",
	"public-source": "Offentlig kilde",
	pilot: "Pilot / avgrenset",
	planned: "Planlagt",
	"system-example": "Systemeksempel",
};

const availabilityGuidance: Record<IntegrationAvailability, string> = {
	available: "Kobles i dag etter at team, arbeidsområde og tilgang er avklart.",
	"public-source": "Brukes som et avgrenset, dokumentert registeroppslag når arbeidet trenger kontroll.",
	pilot: "Vurderes som en avgrenset pilot med konkret arbeidsflyt og tydelig omfang.",
	planned: "Er planlagt arbeid og vurderes sammen med dere før noe tas i bruk.",
	"system-example": "Viser hvilken kontekst systemet normalt kan bidra med; tilgjengelighet og omfang avklares per arbeidsflyt.",
};

const contextUses = [
	"Samle et avgrenset grunnlag i den arbeidsflaten teamet allerede bruker.",
	"La kilden følge svar, utkast og oppfølging når den trengs.",
	"Hold mennesker i kontroll over hva som brukes og hva som blir neste steg.",
] as const;

function requestedIntegrationFromLocation(): string | null {
	if (typeof window === "undefined") return null;
	const integrationId = new URLSearchParams(window.location.search).get("integration");
	return INTEGRATION_CATALOG.some((integration) => integration.id === integrationId) ? integrationId : null;
}

function IntegrationMark({ integration }: { integration: Integration }) {
	if (integration.logo) {
		return (
			<span aria-hidden="true" className="grid size-11 shrink-0 place-items-center rounded-[13px] border border-verevon-j-text/8 bg-white p-2 shadow-[0_8px_18px_rgba(23,23,23,0.035)]">
				<Image
					alt=""
					className="max-h-full max-w-full object-contain"
					height={26}
					src={`/brand-logos/${integration.logo}`}
					width={26}
				/>
			</span>
		);
	}

	return (
		<span aria-hidden="true" className="grid size-11 shrink-0 place-items-center rounded-[13px] border border-verevon-j-text/8 bg-white px-1 text-center font-manrope text-[0.52rem] font-medium uppercase leading-[1.02] tracking-[-0.03em] text-verevon-j-text/72 shadow-[0_8px_18px_rgba(23,23,23,0.035)]">
			{integration.mark}
		</span>
	);
}

export function IntegrationDirectory() {
	const [category, setCategory] = useState<IntegrationCategory | "all">("all");
	const [query, setQuery] = useState("");
	const [selectedId, setSelectedId] = useState<string | null>(null);

	useEffect(() => {
		const requestedId = requestedIntegrationFromLocation();
		if (!requestedId) return;
		const requestedIntegration = INTEGRATION_CATALOG.find((integration) => integration.id === requestedId);
		if (!requestedIntegration) return;
		setCategory(requestedIntegration.category);
		setQuery("");
		setSelectedId(requestedIntegration.id);
	}, []);

	const visibleIntegrations = useMemo(() => {
		const searchTerm = query.trim().toLocaleLowerCase("nb-NO");
		return INTEGRATION_CATALOG.filter((integration) => {
			const matchesCategory = category === "all" || integration.category === category;
			const haystack = `${integration.name} ${integration.provides} ${categoryLabels[integration.category]}`.toLocaleLowerCase("nb-NO");
			return matchesCategory && (!searchTerm || haystack.includes(searchTerm));
		});
	}, [category, query]);

	const selectedIntegration = visibleIntegrations.find((item) => item.id === selectedId) ?? visibleIntegrations[0] ?? null;

	return (
		<section aria-labelledby="integrations-directory-title" className="px-[var(--verevon-edge)] pb-[clamp(78px,10vw,150px)] pt-[clamp(54px,6vw,92px)] max-[760px]:px-[var(--verevon-page-pad)]" id="katalog">
			<div className="mx-auto max-w-[1680px]">
				<div className="grid gap-8 border-b border-verevon-j-text/12 pb-10 lg:grid-cols-[minmax(0,0.92fr)_minmax(420px,0.72fr)] lg:items-end">
					<div>
						<p className="verevon-eyebrow !text-[#686867]">Integrasjonskatalog</p>
						<h2 className="verevon-h2 mt-5 max-w-[11ch] text-balance" id="integrations-directory-title">
							Velg grunnlaget som betyr noe i arbeidet.
						</h2>
					</div>
					<div className="relative">
						<label className="sr-only" htmlFor="integration-search">Søk etter integrasjon</label>
						<Search aria-hidden="true" className="pointer-events-none absolute left-4 top-1/2 size-4 -translate-y-1/2 text-verevon-j-text/48" strokeWidth={1.5} />
						<input
							className="h-13 w-full rounded-full border border-verevon-j-text/14 bg-white pl-11 pr-5 font-geist text-[0.92rem] font-light text-verevon-j-text outline-none transition-[border-color,box-shadow] placeholder:text-verevon-j-text/42 focus:border-verevon-j-text/38 focus:shadow-[0_0_0_4px_rgba(23,23,23,0.05)]"
							id="integration-search"
							onInput={(event) => setQuery(event.currentTarget.value)}
							placeholder="Søk etter system eller område"
							type="search"
							value={query}
						/>
					</div>
				</div>

				<p className="m-0 mt-6 max-w-[70ch] font-geist text-[0.8rem] font-light leading-[1.5] text-verevon-j-text/56">Katalogen skiller mellom systemer som kan kobles i dag, offentlige kilder, avgrensede piloter, planlagt arbeid og systemeksempler.</p>
				<fieldset className="mt-5 max-w-full overflow-x-auto border-0 p-0 pb-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
					<legend className="sr-only">Filtrer integrasjoner</legend>
					<div className="inline-flex min-w-max items-center rounded-[14px] border border-verevon-j-text/9 bg-[#eff1f0] p-1.5 shadow-[0_10px_22px_rgba(23,23,23,0.025)]">
						<IntegrationFilterOption category={category} label="Alle" onChange={setCategory} value="all" />
						{INTEGRATION_CATEGORIES.map((item) => (
							<IntegrationFilterOption category={category} key={item} label={categoryLabels[item]} onChange={setCategory} value={item} />
						))}
					</div>
				</fieldset>

				<div className="mt-7 grid gap-5 xl:grid-cols-[minmax(0,1fr)_minmax(290px,0.34fr)]">
					<div className="grid gap-2.5 sm:grid-cols-2 xl:grid-cols-3" id="integration-results">
						{visibleIntegrations.map((integration) => {
							const selected = selectedIntegration?.id === integration.id;
							return (
								<button
									aria-label={`Vis hva ${integration.name} kan gi Verevon av kontekst`}
									className={`group min-h-[140px] rounded-[20px] border p-4 text-left transition-[border-color,background-color,box-shadow,transform] duration-300 focus-visible:outline-2 focus-visible:outline-offset-4 ${selected ? "border-verevon-j-text/28 bg-white shadow-[0_18px_32px_rgba(23,23,23,0.07)]" : "border-transparent bg-white/65 hover:-translate-y-0.5 hover:border-verevon-j-text/18 hover:bg-white"}`}
									data-integration-card=""
									data-integration-id={integration.id}
									data-selected={selected}
									key={integration.id}
									onClick={() => setSelectedId(integration.id)}
									type="button"
								>
									<div className="flex items-start justify-between gap-3">
										<IntegrationMark integration={integration} />
										<span className="pt-1 text-right font-geist text-[0.62rem] font-medium uppercase tracking-[0.13em] text-verevon-j-text/42">{availabilityLabels[integration.availability]}</span>
									</div>
									<p className="m-0 mt-5 font-manrope text-[1.04rem] font-normal leading-none tracking-[-0.035em] text-verevon-j-text">{integration.name}</p>
									<p className="m-0 mt-2 line-clamp-2 font-geist text-[0.78rem] font-light leading-[1.42] text-verevon-j-text/58">{integration.provides}</p>
								</button>
							);
						})}
					</div>

					<aside aria-live="polite" className="relative overflow-hidden rounded-[24px] border border-verevon-j-text/10 bg-verevon-j-text p-6 text-white xl:sticky xl:top-[94px] xl:min-h-[520px] xl:self-start" data-integration-context>
						<div aria-hidden="true" className="absolute -right-16 -top-14 size-44 rounded-full border border-white/10" />
						<div aria-hidden="true" className="absolute -bottom-22 -left-20 size-48 rounded-full border border-white/10" />
						{selectedIntegration ? (
							<div className="relative flex h-full flex-col">
								<span className="font-geist text-[0.65rem] font-medium uppercase tracking-[0.18em] text-white/46">Kontekst til Verevon</span>
								<div className="mt-8 flex items-center gap-4">
									<IntegrationMark integration={selectedIntegration} />
									<p className="m-0 max-w-[12ch] font-manrope text-[clamp(1.7rem,2.1vw,2.3rem)] font-light leading-[0.96] tracking-[-0.06em]">{selectedIntegration.name}</p>
								</div>
								<p className="m-0 mt-5 max-w-[28ch] font-geist text-[0.94rem] font-light leading-[1.48] text-white/68">{selectedIntegration.provides}</p>
								<div className="mt-7 flex flex-wrap gap-2">
									<span className="inline-flex rounded-full border border-white/16 px-3 py-1.5 font-geist text-[0.66rem] font-medium uppercase tracking-[0.1em] text-white/62">{categoryLabels[selectedIntegration.category]}</span>
									<span className="inline-flex rounded-full border border-white/16 px-3 py-1.5 font-geist text-[0.66rem] font-medium uppercase tracking-[0.1em] text-white/62">{availabilityLabels[selectedIntegration.availability]}</span>
								</div>
								<div className="mt-7 border-y border-white/14 py-6">
									<p className="m-0 font-geist text-[0.65rem] font-medium uppercase tracking-[0.16em] text-white/48">Slik bruker dere konteksten</p>
									<ul className="m-0 mt-4 grid list-none gap-3 p-0">
										{contextUses.map((item) => (
											<li className="flex gap-3 font-geist text-[0.78rem] font-light leading-[1.42] text-white/72" key={item}>
												<span aria-hidden="true" className="mt-0.5 grid size-[17px] shrink-0 place-items-center rounded-full bg-white text-verevon-j-text"><Check className="size-3" strokeWidth={2.4} /></span>
												<span>{item}</span>
											</li>
										))}
									</ul>
								</div>
								<div className="mt-auto pt-7">
									<p className="m-0 font-geist text-[0.74rem] font-light leading-[1.45] text-white/58">{availabilityGuidance[selectedIntegration.availability]}</p>
									<ArrowButton className="mt-5 w-full" href="/#kontakt" variant="light">
										Vurder oppsett
									</ArrowButton>
								</div>
							</div>
						) : (
							<div className="relative grid min-h-[250px] place-items-center text-center font-geist text-[0.9rem] font-light leading-[1.5] text-white/62">Ingen systemer matcher søket.</div>
						)}
					</aside>
				</div>
				<p className="m-0 mt-5 font-geist text-[0.78rem] font-light text-verevon-j-text/48" role="status">
					{visibleIntegrations.length} {visibleIntegrations.length === 1 ? "system" : "systemer"} vist
				</p>
			</div>
		</section>
	);
}

function IntegrationFilterOption({
	category,
	label,
	onChange,
	value,
}: {
	category: IntegrationCategory | "all";
	label: string;
	onChange: (value: IntegrationCategory | "all") => void;
	value: IntegrationCategory | "all";
}) {
	const selected = category === value;
	const id = `integration-filter-${value}`;
	return (
		<label className={`cursor-pointer rounded-[10px] px-3.5 py-2 font-geist text-[0.72rem] font-medium uppercase tracking-[0.055em] transition-[background-color,color,box-shadow] focus-within:outline focus-within:outline-2 focus-within:outline-offset-2 focus-within:outline-verevon-j-text ${selected ? "bg-verevon-j-text text-white shadow-[0_2px_8px_rgba(23,23,23,0.14)]" : "text-verevon-j-text/58 hover:bg-white/70 hover:text-verevon-j-text"}`} htmlFor={id}>
			<input
				checked={selected}
				className="sr-only"
				id={id}
				name="integration-filter"
				onChange={() => onChange(value)}
				type="radio"
				value={value}
			/>
			<span>{label}</span>
		</label>
	);
}

export default IntegrationDirectory;
