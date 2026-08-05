/**
 * WordmarkSectionV2 — typographic confidence (911 Rennsport pattern).
 *
 * One oversized VEREVON wordmark as a design element, in place of more
 * animation. Sits before the (untouched) pre-footer.
 */
export function WordmarkSectionV2() {
	return (
		<section className="relative overflow-hidden border-t border-verevon-j-text/8 bg-background px-[var(--verevon-edge)] pb-[clamp(28px,3.5vw,56px)] pt-[clamp(72px,8vw,128px)] text-verevon-j-text max-[760px]:px-[var(--verevon-page-pad)]">
			<div aria-hidden="true" className="pointer-events-none select-none">
				<span className="block w-full text-center font-arbeit text-[18vw] font-light leading-[0.78] tracking-[-0.045em] text-verevon-j-text/90 max-[760px]:text-[26vw]">
					VEREVON
				</span>
			</div>
		</section>
	);
}

export default WordmarkSectionV2;
