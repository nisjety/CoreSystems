import { ArrowButton } from "@/components/ui/buttons/ArrowButton";

export function HeroSection() {
  return (
    <section className="velion-hero" data-hero-parallax="" id="top">
      <div
        aria-hidden="true"
        className="velion-hero__media"
        data-hero-parallax-media=""
        data-parallax-effect=""
        data-parallax-options='{"from":{"y":"0%"},"to":{"y":"80%"},"start":"top top","end":"bottom top","disableOnMobile":false,"disableOnTablet":false}'
      >
        <div className="velion-water">
          <div className="velion-vessel">
            <span className="velion-vessel__roof" />
            <span className="velion-vessel__glass" />
            <span className="velion-vessel__deck" />
          </div>
          <span className="velion-wake velion-wake--left" />
          <span className="velion-wake velion-wake--right" />
        </div>
      </div>

      <div
        className="velion-hero__content"
        data-hero-parallax-content=""
        data-parallax-effect=""
        data-parallax-options='{"from":{"y":"0%"},"to":{"y":"40%"},"start":"top top","end":"bottom top","disableOnMobile":false,"disableOnTablet":false}'
      >
        <h1>Electric Luxury</h1>
        <p>
          Experience style, sustainability and forward-thinking. Ride the VELION.
        </p>
        <ArrowButton href="#yachts" variant="light">
          Discover Our Yachts
        </ArrowButton>
      </div>

      <a className="velion-hero__explore" href="#yachts">
        <svg aria-hidden="true" viewBox="0 0 18 18">
          <path d="m3 6 6 6 6-6" />
        </svg>
        Explore
      </a>
    </section>
  );
}
