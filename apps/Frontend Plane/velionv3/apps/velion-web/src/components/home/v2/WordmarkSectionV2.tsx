/**
 * WordmarkSectionV2 — typographic confidence (911 Rennsport pattern).
 *
 * One oversized VELION wordmark as a design element, in place of more
 * animation. Sits before the (untouched) pre-footer.
 */
export function WordmarkSectionV2() {
	return (
		<section className="relative overflow-hidden border-t border-velion-j-text/8 bg-background px-[var(--velion-edge)] pb-[clamp(28px,3.5vw,56px)] pt-[clamp(72px,8vw,128px)] text-velion-j-text max-[760px]:px-[var(--velion-page-pad)]">
			<div aria-hidden="true" className="pointer-events-none select-none">
				<span className="block w-full text-center font-arbeit text-[18vw] font-light leading-[0.78] tracking-[-0.045em] text-velion-j-text/90 max-[760px]:text-[26vw]">
					VELION
				</span>
			</div>
		</section>
	);
}

export default WordmarkSectionV2;
