export function IntroStepContent() {
  return (
    <section class="onboarding-copy onboarding-copy--intro">
      <p class="onboarding-eyebrow">Intro</p>
      <h1>Alt Er Klart</h1>
      <p>Vi forbereder arbeidsplassen din. Det tar et øyeblikk.</p>
      <div class="onboarding-spinner-row">
        <span class="onboarding-spinner" aria-hidden="true" />
        Setter opp Verevon ...
      </div>
    </section>
  )
}

export function IntroStepVisual(props: { onEnded: () => void }) {
  return (
    <div class="onboarding-product-reveal">
      <video
        src="/videos/onboarding/product-reveal.webm"
        poster="/imagens/onboarding/product-reveal-poster.png"
        autoplay
        muted
        playsinline
        onEnded={() => props.onEnded()}
      />
      <div class="onboarding-product-reveal__overlay">
        <p>Verevon · for team som svarer.</p>
        <span>42 % raskere første svar · 18 språk · alt på din kunnskap.</span>
      </div>
    </div>
  )
}
