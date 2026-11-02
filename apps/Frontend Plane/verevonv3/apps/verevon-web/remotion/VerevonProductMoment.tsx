import type { CSSProperties, ReactNode } from "react";
import { AbsoluteFill, Easing, interpolate, useCurrentFrame } from "remotion";
import { COLORS, FONT_CSS, geist, manrope } from "./theme";

export type ProductMomentKind = "followUp" | "investigate" | "plan";

const easeOut = Easing.bezier(0.16, 1, 0.3, 1);

const reveal = (frame: number, start: number, length = 18) =>
	interpolate(frame, [start, start + length], [0, 1], {
		easing: easeOut,
		extrapolateLeft: "clamp",
		extrapolateRight: "clamp",
	});

function Enter({
	children,
	delay,
	distance = 18,
	style,
}: {
	children: ReactNode;
	delay: number;
	distance?: number;
	style?: CSSProperties;
}) {
	const frame = useCurrentFrame();
	const progress = reveal(frame, delay);

	return (
		<div
			style={{
				opacity: progress,
				transform: `translateY(${(1 - progress) * distance}px) scale(${0.985 + progress * 0.015})`,
				...style,
			}}
		>
			{children}
		</div>
	);
}

function StatusDot({ color = COLORS.coral }: { color?: string }) {
	return <span style={{ background: color, borderRadius: 999, display: "inline-block", height: 8, width: 8 }} />;
}

function SourceChip({ label, tone = "neutral" }: { label: string; tone?: "accent" | "neutral" }) {
	return (
		<span
			style={{
				alignItems: "center",
				background: tone === "accent" ? "#fff0e8" : "#f5f4f0",
				border: `1px solid ${tone === "accent" ? "rgba(238,122,80,0.24)" : COLORS.line}`,
				borderRadius: 999,
				color: tone === "accent" ? COLORS.coralDeep : COLORS.muted,
				display: "inline-flex",
				fontSize: 12,
				gap: 7,
				padding: "7px 10px",
				whiteSpace: "nowrap",
				...geist,
			}}
		>
			<StatusDot color={tone === "accent" ? COLORS.coral : "#a19c93"} />
			{label}
		</span>
	);
}

function CheckRow({
	children,
	active,
	delay,
}: {
	children: ReactNode;
	active?: boolean;
	delay: number;
}) {
	const frame = useCurrentFrame();
	const progress = reveal(frame, delay, 14);

	return (
		<div
			style={{
				alignItems: "center",
				background: active ? "#fffaf6" : "transparent",
				border: active ? "1px solid rgba(238,122,80,0.16)" : "1px solid transparent",
				borderRadius: 11,
				display: "flex",
				fontSize: 15,
				gap: 11,
				opacity: progress,
				padding: "12px 13px",
				transform: `translateX(${(1 - progress) * 16}px)`,
				...geist,
			}}
		>
			<span
				style={{
					alignItems: "center",
					background: progress > 0.92 ? COLORS.success : "#e6e3dd",
					borderRadius: 999,
					color: "white",
					display: "inline-flex",
					fontSize: 12,
					height: 20,
					justifyContent: "center",
					width: 20,
				}}
			>
				{progress > 0.92 ? "✓" : ""}
			</span>
			{children}
		</div>
	);
}

function AppFrame({
	children,
	label,
}: {
	children: ReactNode;
	label: string;
}) {
	return (
		<AbsoluteFill
			style={{
				background: "radial-gradient(circle at 78% 8%, rgba(238,122,80,0.16), transparent 30%), linear-gradient(135deg, #f1efeb 0%, #f7f6f3 54%, #eeece7 100%)",
				color: COLORS.ink,
				fontFamily: "Geist, Arial, sans-serif",
				padding: 58,
			}}
		>
			<style>{FONT_CSS}</style>
			<div
				style={{
					background: "rgba(255,254,250,0.94)",
					border: "1px solid rgba(23,23,23,0.13)",
					borderRadius: 24,
					boxShadow: "0 36px 80px rgba(37, 33, 28, 0.14)",
					display: "grid",
					gridTemplateRows: "58px 1fr",
					height: "100%",
					overflow: "hidden",
					width: "100%",
				}}
			>
				<header
					style={{
						alignItems: "center",
						borderBottom: `1px solid ${COLORS.line}`,
						display: "flex",
						gap: 13,
						padding: "0 20px",
					}}
				>
					<span style={{ alignItems: "center", background: COLORS.ink, borderRadius: 7, color: "white", display: "flex", fontSize: 13, height: 26, justifyContent: "center", width: 26, ...manrope }}>V</span>
					<span style={{ color: COLORS.ink, fontSize: 13, fontWeight: 600, ...geist }}>Verevon</span>
					<span style={{ color: "#aaa49a", fontSize: 12, ...geist }}> / </span>
					<span style={{ color: COLORS.muted, fontSize: 12, ...geist }}>{label}</span>
					<span style={{ marginLeft: "auto", color: "#8b867e", fontSize: 10, letterSpacing: "0.12em", textTransform: "uppercase", ...geist }}>Eksempelflyt</span>
				</header>
				<div style={{ display: "grid", gridTemplateColumns: "64px 1fr", minHeight: 0 }}>
					<aside style={{ alignItems: "center", background: "rgba(249,248,245,0.9)", borderRight: `1px solid ${COLORS.line}`, display: "flex", flexDirection: "column", gap: 18, paddingTop: 26 }}>
						{[0, 1, 2, 3, 4].map((index) => <span key={index} style={{ background: index === 1 ? COLORS.coral : "#d9d5cd", borderRadius: 5, height: 16, opacity: index === 1 ? 1 : 0.72, width: 16 }} />)}
					</aside>
					{children}
				</div>
			</div>
		</AbsoluteFill>
	);
}

function PlanMoment() {
	return (
		<AppFrame label="Plan">
			<div style={{ display: "grid", gridTemplateColumns: "1.05fr 0.95fr", minHeight: 0 }}>
				<section style={{ padding: "38px 34px" }}>
					<Enter delay={-18}>
						<p style={{ color: COLORS.muted, fontSize: 12, letterSpacing: "0.12em", margin: "0 0 12px", textTransform: "uppercase", ...geist }}>Spørsmål</p>
						<div style={{ background: "#f4f2ed", border: `1px solid ${COLORS.line}`, borderRadius: 14, fontSize: 18, lineHeight: 1.4, padding: "18px 19px", ...geist }}>Lag en realistisk plan for å sikre leveransen.</div>
					</Enter>
					<Enter delay={30} style={{ marginTop: 24 }}>
						<div style={{ color: COLORS.coralDeep, fontSize: 12, fontWeight: 600, ...geist }}>Verevon foreslår et grunnlag</div>
					</Enter>
					<div style={{ marginTop: 10 }}>
						<CheckRow delay={39} active>Avklar ansvar med produkteier</CheckRow>
						<CheckRow delay={56}>Prioriter avhengigheter før eksport</CheckRow>
						<CheckRow delay={73}>Bekreft frist med teamet</CheckRow>
					</div>
					<Enter delay={91} style={{ marginTop: 25 }}>
						<div style={{ alignItems: "center", color: COLORS.muted, display: "flex", fontSize: 12, gap: 9, ...geist }}><StatusDot color={COLORS.success} />3 steg bygget fra avtalt grunnlag</div>
					</Enter>
				</section>
				<aside style={{ background: "#f7f6f2", borderLeft: `1px solid ${COLORS.line}`, padding: "34px 30px" }}>
					<Enter delay={58}>
						<div style={{ background: "white", border: `1px solid ${COLORS.line}`, borderRadius: 16, boxShadow: "0 14px 30px rgba(33,29,25,0.07)", padding: 22 }}>
							<div style={{ alignItems: "center", display: "flex", justifyContent: "space-between" }}><span style={{ color: COLORS.muted, fontSize: 11, letterSpacing: "0.11em", textTransform: "uppercase", ...geist }}>Planutkast</span><span style={{ background: COLORS.coralSoft, borderRadius: 999, color: COLORS.coralDeep, fontSize: 11, padding: "6px 9px", ...geist }}>Utkast</span></div>
							<h2 style={{ fontSize: 30, fontWeight: 350, letterSpacing: "-0.055em", lineHeight: 1.02, margin: "19px 0 20px", ...manrope }}>Klarering før leveranse</h2>
							<div style={{ borderTop: `1px solid ${COLORS.line}`, display: "grid", gap: 14, paddingTop: 17 }}>
								{["Ansvar og mandat", "Frister og avhengigheter", "Eksportkontroll"].map((label, index) => <div key={label} style={{ alignItems: "center", display: "flex", fontSize: 13, gap: 10, ...geist }}><span style={{ background: index < 2 ? COLORS.successSoft : "#f0eee8", borderRadius: 5, color: COLORS.success, display: "grid", height: 22, placeItems: "center", width: 22 }}>{index < 2 ? "✓" : "3"}</span>{label}</div>)}
							</div>
						</div>
					</Enter>
					<Enter delay={114} style={{ marginTop: 17 }}>
						<div style={{ alignItems: "center", background: COLORS.ink, borderRadius: 11, color: "white", display: "flex", fontSize: 13, justifyContent: "center", padding: "13px 15px", ...geist }}>Klar for godkjenning</div>
					</Enter>
				</aside>
			</div>
		</AppFrame>
	);
}

function InvestigateMoment() {
	return (
		<AppFrame label="Undersøk">
			<div style={{ display: "grid", gridTemplateColumns: "1fr 0.94fr", minHeight: 0 }}>
				<section style={{ padding: "37px 34px" }}>
					<Enter delay={-18}>
						<div style={{ background: "#fff7f3", border: "1px solid rgba(190,76,42,0.18)", borderRadius: 14, padding: 18 }}>
							<div style={{ color: "#a84931", fontSize: 11, letterSpacing: "0.11em", textTransform: "uppercase", ...geist }}>Signal oppdaget</div>
							<div style={{ fontSize: 19, lineHeight: 1.25, marginTop: 8, ...geist }}>Eksporten stopper for én kunde.</div>
						</div>
					</Enter>
					<Enter delay={35} style={{ marginTop: 25 }}>
						<p style={{ color: COLORS.muted, fontSize: 12, letterSpacing: "0.12em", margin: 0, textTransform: "uppercase", ...geist }}>Undersøker med</p>
						<div style={{ display: "flex", flexWrap: "wrap", gap: 9, marginTop: 12 }}><SourceChip label="Logg" tone="accent" /><SourceChip label="Kunder" /><SourceChip label="Ordre" /></div>
					</Enter>
					<Enter delay={60} style={{ marginTop: 29 }}>
						<div style={{ borderLeft: `2px solid ${COLORS.coral}`, color: COLORS.ink, fontSize: 16, lineHeight: 1.45, paddingLeft: 15, ...geist }}>Sammenligner kundedata med valideringsloggen og det avtalte skjemaet.</div>
					</Enter>
					<Enter delay={83} style={{ marginTop: 26 }}>
						<div style={{ alignItems: "center", color: COLORS.muted, display: "flex", fontSize: 12, gap: 9, ...geist }}><StatusDot color="#725bd3" />Sporbarhet beholdt gjennom hele undersøkelsen</div>
					</Enter>
				</section>
				<aside style={{ background: "#f7f6f2", borderLeft: `1px solid ${COLORS.line}`, padding: "34px 30px" }}>
					<Enter delay={58}>
						<div style={{ background: "white", border: `1px solid ${COLORS.line}`, borderRadius: 16, boxShadow: "0 14px 30px rgba(33,29,25,0.07)", padding: 22 }}>
							<span style={{ color: COLORS.muted, fontSize: 11, letterSpacing: "0.11em", textTransform: "uppercase", ...geist }}>Funn fra kildene</span>
							<h2 style={{ fontSize: 29, fontWeight: 350, letterSpacing: "-0.055em", lineHeight: 1.02, margin: "18px 0 14px", ...manrope }}>Manglende felt stopper eksporten.</h2>
							<p style={{ color: COLORS.muted, fontSize: 14, lineHeight: 1.45, margin: 0, ...geist }}>Kunde-ID er tom i én ordre. Forslaget er avgrenset før det kan testes.</p>
							<div style={{ background: "#f6f4ef", borderRadius: 11, color: COLORS.muted, fontSize: 12, marginTop: 19, padding: "12px 13px", ...geist }}>Valideringslogg · rad 18<br /><span style={{ color: COLORS.ink }}>customer_id mangler</span></div>
						</div>
					</Enter>
					<Enter delay={116} style={{ marginTop: 17 }}>
						<div style={{ alignItems: "center", background: "#eef5f0", border: "1px solid rgba(54,117,90,0.18)", borderRadius: 11, color: "#2b684d", display: "flex", fontSize: 13, justifyContent: "center", padding: "13px 15px", ...geist }}>Endring klar for test</div>
					</Enter>
				</aside>
			</div>
		</AppFrame>
	);
}

function FollowUpMoment() {
	return (
		<AppFrame label="Følg opp">
			<div style={{ display: "grid", gridTemplateColumns: "1.05fr 0.95fr", minHeight: 0 }}>
				<section style={{ padding: "37px 34px" }}>
					<Enter delay={-18}>
						<div style={{ background: "#f4f2ed", border: `1px solid ${COLORS.line}`, borderRadius: 14, fontSize: 18, lineHeight: 1.4, padding: "18px 19px", ...geist }}>Lag et kundeutkast basert på status og tidligere dialog.</div>
					</Enter>
					<Enter delay={34} style={{ marginTop: 18 }}>
						<div style={{ display: "flex", flexWrap: "wrap", gap: 9 }}><SourceChip label="Bring-sporing" tone="accent" /><SourceChip label="Tidligere dialog" /><SourceChip label="Returpolicy" /></div>
					</Enter>
					<Enter delay={57} style={{ marginTop: 23 }}>
						<div style={{ background: "white", border: `1px solid ${COLORS.line}`, borderRadius: 14, boxShadow: "0 10px 24px rgba(33,29,25,0.06)", padding: 18 }}>
							<div style={{ color: COLORS.coralDeep, fontSize: 11, fontWeight: 600, letterSpacing: "0.1em", textTransform: "uppercase", ...geist }}>Utkast</div>
							<p style={{ fontSize: 16, lineHeight: 1.45, margin: "10px 0 0", ...geist }}>«Vi ser at sendingen er forsinket. Her er status og neste steg for deg.»</p>
						</div>
					</Enter>
					<Enter delay={86} style={{ marginTop: 19 }}>
						<div style={{ alignItems: "center", color: COLORS.muted, display: "flex", fontSize: 12, gap: 9, ...geist }}><StatusDot color={COLORS.success} />Svar og beslutning har samme kildegrunnlag</div>
					</Enter>
				</section>
				<aside style={{ background: "#f7f6f2", borderLeft: `1px solid ${COLORS.line}`, padding: "34px 30px" }}>
					<Enter delay={68}>
						<div style={{ background: "white", border: `1px solid ${COLORS.line}`, borderRadius: 16, boxShadow: "0 14px 30px rgba(33,29,25,0.07)", padding: 22 }}>
							<div style={{ alignItems: "center", display: "flex", justifyContent: "space-between" }}><span style={{ color: COLORS.muted, fontSize: 11, letterSpacing: "0.11em", textTransform: "uppercase", ...geist }}>Gjennomgang</span><span style={{ color: COLORS.success, fontSize: 12, ...geist }}>2 kilder</span></div>
							<h2 style={{ fontSize: 29, fontWeight: 350, letterSpacing: "-0.055em", lineHeight: 1.02, margin: "18px 0 20px", ...manrope }}>Klar for neste steg</h2>
							<div style={{ borderTop: `1px solid ${COLORS.line}`, display: "grid", gap: 11, paddingTop: 15 }}>
								{["Kilder kontrollert", "Utkast gjennomgått", "Oppfølging valgt"].map((label, index) => <CheckRow active delay={82 + index * 14} key={label}>{label}</CheckRow>)}
							</div>
						</div>
					</Enter>
					<Enter delay={132} style={{ marginTop: 17 }}>
						<div style={{ alignItems: "center", background: COLORS.ink, borderRadius: 11, color: "white", display: "flex", fontSize: 13, justifyContent: "center", padding: "13px 15px", ...geist }}>Klargjort for oppfølging</div>
					</Enter>
				</aside>
			</div>
		</AppFrame>
	);
}

export function VerevonProductMoment({ kind }: { kind: ProductMomentKind }) {
	const scene = kind === "plan" ? <PlanMoment /> : kind === "investigate" ? <InvestigateMoment /> : <FollowUpMoment />;

	return <AbsoluteFill>{scene}</AbsoluteFill>;
}
