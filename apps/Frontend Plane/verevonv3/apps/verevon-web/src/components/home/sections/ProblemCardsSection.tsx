"use client";

import { useState, type PointerEvent } from "react";
import Link from "next/link";
import {
	KnowledgeCardVisual,
	type KnowledgeCardVisual as KnowledgeCardVisualType,
} from "@/components/home/sections/EndToEndKnowledgeVisual";
import { Reveal } from "@/components/home/sections/Reveal";
import { ArrowButton, ArrowButtonLabel } from "@/components/ui/ArrowButton";
import { cn } from "@/lib/utils";
import styles from "./KnowledgeCards.module.css";

type KnowledgeCard = {
	body: string;
	href: string;
	label: string;
	number: string;
	title: string;
	type: KnowledgeCardVisualType;
};

const cards: KnowledgeCard[] = [
	{
		number: "01",
		title: "Koble til systemene dere bruker.",
		body: "Koble dokumenter, data og verktøy fra systemene dere allerede bruker. Kunnskapen blir tilgjengelig der arbeidet skjer.",
		href: "/integrasjoner",
		label: "Se integrasjonene",
		type: "connect",
	},
	{
		number: "02",
		title: "Se sammenhengen.",
		body: "Knytt kunnskap på tvers av avdelinger, slik at svar, planer og handlinger bygger på samme sporbare grunnlag.",
		href: "/plattform/felles-kontekst#kilder",
		label: "Utforsk kunnskapen",
		type: "knowledge",
	},
	{
		number: "03",
		title: "Verevon. Deres virksomhet.",
		body: "Én arbeidsflate for mennesker og agenter. Still spørsmål, sett arbeid i gang og følg resultatet på tvers av systemene.",
		href: "/plattform/kontrollert-arbeid",
		label: "Se plattformen",
		type: "business",
	},
];

function KnowledgeNavigation({
	active,
	onPointerEnter,
	onFocus,
}: {
	active: string | null;
	onPointerEnter: (event: PointerEvent<HTMLElement>, number: string) => void;
	onFocus: (number: string) => void;
}) {
	return (
		<nav
			aria-label="Utforsk Verevon"
			className={styles.sectionNavigation}
			data-navigation-style="stingray"
		>
			<ul>
				{cards.map((card) => (
					<li key={card.number}>
						<Link
							href={card.href}
							data-highlighted={active === card.number}
							data-navigation-link="stingray"
							onPointerEnter={(event) => onPointerEnter(event, card.number)}
							onFocus={() => onFocus(card.number)}
						>
							{card.title}
						</Link>
					</li>
				))}
			</ul>
		</nav>
	);
}

export function ProblemCardsSection() {
	const [hovered, setHovered] = useState<string | null>(null);
	const [opened, setOpened] = useState<string | null>(null);
	const active = hovered ?? opened;

	function previewCard(event: PointerEvent<HTMLElement>, number: string) {
		if (event.pointerType === "mouse" && window.matchMedia("(min-width: 1024px) and (hover: hover)").matches) {
			setHovered(number);
		}
	}

	return (
		<section
			aria-labelledby="end-to-end-knowledge-title"
			className="verevon-solution-bridge relative isolate overflow-visible px-[var(--verevon-edge)] py-[clamp(70px,8.8vw,140px)] text-verevon-j-text max-[760px]:px-[var(--verevon-page-pad)]"
			data-problem-cards=""
			id="problemomrader"
		>
			<div aria-hidden="true" className="verevon-solution-bridge-rule" />

			<Reveal delay={90}>
				<header className="mx-auto max-w-[770px] text-center max-[760px]:text-left">
					<h2
						className="font-manrope text-[clamp(2rem,3.2vw,3.5rem)] font-light leading-[0.98] tracking-[-0.055em] text-pretty"
						id="end-to-end-knowledge-title"
					>
						<span className="block">Koble virksomheten</span>{" "}
						<span className="block">sammen igjen.</span>
					</h2>
					<p className="verevon-body mx-auto mt-5 max-w-[58ch] text-verevon-text-muted text-pretty max-[760px]:mx-0">
						Verevon samler systemer, kunnskap og arbeid i én felles arbeidsflate.
						Fagsystemene blir stående.
					</p>
				</header>
			</Reveal>

			<Reveal delay={170}>
				<div
					onPointerLeave={() => setHovered(null)}
					onBlur={(event) => {
						if (!event.currentTarget.contains(event.relatedTarget)) setOpened(null);
					}}
					onKeyDown={(event) => {
						if (event.key === "Escape") { setHovered(null); setOpened(null); }
						if (event.key === "Tab") setHovered(null);
					}}
				>
					<div className={styles.cards} data-active={active ?? "none"}>
						{cards.map((card) => {
							const isBusiness = card.type === "business";
							const navigatesWholeCard = card.type === "knowledge" || isBusiness;
							const expanded = active === card.number;
							const cardContent = (
								<>
									<span className={styles.number}>{card.number}</span>
									{!navigatesWholeCard ? <button
										aria-controls={`knowledge-visual-${card.number}`}
										aria-expanded={expanded}
										aria-label={`${expanded ? "Lukk" : "Utvid"} kort ${card.number}`}
										className={styles.expand}
										type="button"
										onClick={() => {
											setHovered(null);
											setOpened(expanded ? null : card.number);
										}}
									>
										<svg aria-hidden="true" viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth="1.2"><path d={expanded ? "M3 8h5V3M17 12h-5v5" : "M3 8V3h5M17 12v5h-5"} /></svg>
									</button> : null}
									{isBusiness ? (
										<div id={`knowledge-visual-${card.number}`}><KnowledgeCardVisual expanded={expanded} type={card.type} /></div>
									) : (
										<div className={styles.visual} id={`knowledge-visual-${card.number}`}>
											<KnowledgeCardVisual expanded={expanded} type={card.type} />
										</div>
									)}
									<div className={styles.content}>
										<h3 id={`knowledge-card-${card.number}`}>{card.title}</h3>
										<p>{card.body}</p>
										{navigatesWholeCard ? (
											<span aria-hidden="true" className={cn(styles.cta, isBusiness ? "text-verevon-c-white" : "text-verevon-j-text")}>
												<ArrowButtonLabel>{card.label}</ArrowButtonLabel>
											</span>
										) : (
											<ArrowButton className={styles.cta} href={card.href} variant="dark">
												{card.label}
											</ArrowButton>
										)}
									</div>
								</>
							);

							if (navigatesWholeCard) {
								return (
									<article
										aria-labelledby={`knowledge-card-${card.number}`}
										className={cn(styles.card, styles[card.type], "verevon-cta-card")}
										data-expanded={expanded}
										data-muted={active !== null && !expanded}
										key={card.number}
										onPointerEnter={(event) => previewCard(event, card.number)}
									>
										<Link
											aria-label={`${card.label}: ${card.title}`}
											className={styles.cardLink}
											data-card-link={card.type}
											href={card.href}
										>
											<span className="sr-only">{card.label}</span>
										</Link>
										{cardContent}
									</article>
								);
							}

							return (
								<article
									aria-labelledby={`knowledge-card-${card.number}`}
									className={cn(styles.card, styles[card.type])}
									data-expanded={expanded}
									data-muted={active !== null && !expanded}
									key={card.number}
									onPointerEnter={(event) => previewCard(event, card.number)}
								>
									{cardContent}
								</article>
							);
						})}
					</div>
					<KnowledgeNavigation
						active={active}
						onPointerEnter={previewCard}
						onFocus={(number) => {
							setHovered(null);
							setOpened(number);
						}}
					/>
				</div>
			</Reveal>
		</section>
	);
}

export default ProblemCardsSection;
