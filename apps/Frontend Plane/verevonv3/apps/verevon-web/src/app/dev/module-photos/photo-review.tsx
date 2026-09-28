"use client";

import { useState } from "react";
import Link from "next/link";
import { ModuleWorkflowCard } from "@/components/home/sections/FeatureWorkflowCards";
import { moduleCards } from "@/components/home/sections/feature-workflow-cards";
import { modulePhotoOptions, modulePhotoOrder } from "@/components/home/sections/module-photo-options";

const proposedChoices = [9, 5, 3, 0, 1];
const homepageChoices = modulePhotoOrder.map((key, index) => modulePhotoOptions[key].findIndex((photo) => photo.image === moduleCards[index].image));
const combinations = [
	{ name: "Dokumenter og samtale", choices: proposedChoices, description: "Kunnskap 10 + AI og agenter 6. Kildene ligger nær, og en tydelig samtale peker mot neste steg." },
	{ name: "Uten glass · overlevering", choices: [9, 10, 3, 0, 1], description: "Kunnskap 10 + AI og agenter 11. Nært på lesing av kilder og en tydelig overlevering over arbeidsbordet." },
	{ name: "Gjennom glass", choices: [6, 5, 3, 0, 1], description: "Kunnskap 7 + AI og agenter 6. Felles lesing og en forklarende samtale, begge sett gjennom glass." },
	{ name: "Kilder og overlevering", choices: [7, 8, 3, 0, 1], description: "Kunnskap 8 + AI og agenter 9. Dokumenter undersøkes, og en mappe gis videre til neste person." },
	{ name: "Søk og samtale", choices: [4, 6, 3, 0, 1], description: "Kunnskap 5 + AI og agenter 7. Bibliotekleseren fra listen din og en samtale gjennom glass fra Unsplash." },
	{ name: "Mapper og oppdrag", choices: [1, 7, 3, 0, 1], description: "Kunnskap 2 + AI og agenter 8. Mappebildet beholdt som stilreferanse, sammen med møtemotivet du foreslo." },
];

function photoStatus(key: typeof modulePhotoOrder[number], cardIndex: number, choice: number) {
	if (cardIndex >= 2 && choice === proposedChoices[cardIndex]) return "Valgt av deg";
	if (key === "knowledge" && choice === 1) return "Beholdt som stilreferanse";
	if ((key === "knowledge" && choice >= 8) || (key === "agents" && choice >= 9)) return "Nytt · uten glass";
	if ((key === "knowledge" && choice >= 4) || (key === "agents" && choice >= 5)) return "Nytt forslag";
	if (key === "agents") return "Tidligere retning · transit";
	return choice === homepageChoices[cardIndex] ? "På forsiden" : "Alternativ";
}

export function ModulePhotoReview() {
	const [choices, setChoices] = useState<number[]>(proposedChoices);
	const selectedCombination = combinations.find((combination) => combination.choices.every((choice, index) => choice === choices[index]));

	return (
		<main className="min-h-screen bg-[#f8f8f7] px-6 py-9 font-protokoll text-verevon-j-text md:px-9">
			<header className="mx-auto mb-12 max-w-[1600px]">
				<Link className="text-sm underline underline-offset-4" href="/#features">← Til forsiden</Link>
				<div className="mt-10 flex flex-wrap items-end justify-between gap-6">
					<div>
						<p className="mb-3 text-xs tracking-[0.16em] text-verevon-text-muted">VEREVON / BILDEVALG</p>
						<h1 className="font-arbeit text-[clamp(2rem,3.5vw,3.8rem)] font-light leading-none tracking-[-0.045em]">Fem områder. Én sammenheng.</h1>
		<p className="mt-4 max-w-2xl text-sm leading-relaxed text-verevon-text-muted">Arbeidsflate 4, Datamaskiner og support 1 og Tilgang og kontroll 2 er valgt. Kunnskap 10 og AI og agenter 6 viser de tydeligste bildene på kilder og delegering.</p>
					</div>
					<div className="flex flex-wrap gap-3 text-sm">
						<button className="min-h-11 cursor-pointer border border-verevon-j-text/20 px-4 hover:bg-verevon-j-text/5 focus-visible:outline-2 focus-visible:outline-offset-4" onClick={() => setChoices([...homepageChoices])} type="button">Forsidens bilder</button>
						<button className="min-h-11 cursor-pointer border border-verevon-j-text/20 px-4 hover:bg-verevon-j-text/5 focus-visible:outline-2 focus-visible:outline-offset-4" onClick={() => setChoices((current) => current.map((choice, index) => index === 0 ? 1 : choice))} type="button">Prøv mappebildet</button>
					</div>
				</div>
				<div className="mt-7 grid grid-cols-2 gap-2 lg:grid-cols-3" role="group" aria-label="Prøv en bildekombinasjon">
					{combinations.map((combination) => (
						<button
							aria-pressed={selectedCombination === combination}
							className={`min-h-11 cursor-pointer border border-verevon-j-text/20 px-4 py-3 text-left text-sm transition-colors focus-visible:outline-2 focus-visible:outline-offset-4 ${selectedCombination === combination ? "bg-verevon-j-text text-white" : "hover:bg-verevon-j-text/5"}`}
							key={combination.name}
							onClick={() => setChoices([...combination.choices])}
							type="button"
						>
							{combination.name}
						</button>
					))}
				</div>
				<p className="mt-3 min-h-10 text-xs leading-relaxed text-verevon-text-muted" aria-live="polite">{selectedCombination?.description ?? "Din egen kombinasjon."} Alle kombinasjonene beholder de tre siste bildevalgene dine. Knappevalgene gjelder bare forhåndsvisningen.</p>
			</header>

			<div className="mx-auto grid max-w-[1600px] grid-cols-1 gap-x-6 gap-y-12 sm:grid-cols-2 xl:grid-cols-5">
				{modulePhotoOrder.map((key, index) => {
					const options = modulePhotoOptions[key];
					const selected = choices[index] ?? 0;
					const photo = options[selected] ?? options[0];
					const card = { ...moduleCards[index], ...photo };

					return (
						<section className="flex min-w-0 flex-col" aria-label={`Bildevalg for ${card.area}`} key={key}>
							<div className="mb-2 grid min-h-33 grid-cols-5 content-start border border-verevon-j-text/20" role="group" aria-label={`Velg bilde for ${card.area}`}>
								{options.map((option, choice) => (
									<button
										aria-label={`${card.area}: bilde ${choice + 1}`}
										aria-pressed={selected === choice}
										className={`min-h-11 flex-1 cursor-pointer px-2 text-xs transition-colors focus-visible:z-10 focus-visible:outline-2 focus-visible:outline-offset-4 ${selected === choice ? "bg-verevon-j-text text-white" : "hover:bg-verevon-j-text/5"}`}
										key={option.image}
										onClick={() => setChoices((current) => current.map((value, cardIndex) => cardIndex === index ? choice : value))}
										type="button"
									>
										{choice + 1}
									</button>
								))}
							</div>
							<p className="mb-5 text-xs text-verevon-text-muted">{selected + 1} · {photoStatus(key, index, selected)}</p>
							<ModuleWorkflowCard animated={false} card={card} index={index} total={moduleCards.length} className="flex-1" />
							<div className="mt-6 border-t border-verevon-j-text/15 pt-4 text-xs leading-relaxed text-verevon-text-muted">
								<p className="min-h-[8em]">{photo.note}</p>
								<a className="mt-3 block underline underline-offset-4 hover:text-verevon-j-text" href={photo.source} target="_blank" rel="noreferrer">{photo.credit} ↗</a>
								<p className="mt-1">Original: {photo.imageWidth} × {photo.imageHeight} px</p>
							</div>
						</section>
					);
				})}
			</div>
			<p className="mx-auto mt-12 max-w-[1600px] border-t border-verevon-j-text/15 pt-5 text-sm text-verevon-text-muted" role="status">Ditt bildevalg: {choices.map((choice) => choice + 1).join(" · ")} — Kunnskap, AI og agenter, Arbeidsflate, Datamaskiner og support, Tilgang og kontroll.</p>
		</main>
	);
}
