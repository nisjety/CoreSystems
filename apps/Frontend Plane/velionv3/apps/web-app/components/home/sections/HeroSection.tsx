import Image from "next/image";
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
        <Image
          alt=""
          className="velion-hero__image"
          fill
          priority
          sizes="100vw"
          src="/velion-vibe/human-haze.png"
        />
        <div className="velion-hero__veil" />
      </div>

      <div
        className="velion-hero__content"
        data-hero-parallax-content=""
        data-parallax-effect=""
        data-parallax-options='{"from":{"y":"0%"},"to":{"y":"40%"},"start":"top top","end":"bottom top","disableOnMobile":false,"disableOnTablet":false}'
      >
        <h1>Your AI worker for customer experience.</h1>
        <p>
          Velion learns the company, drafts work, asks for approval, and keeps every customer action traceable.
        </p>
        <ArrowButton href="#product-loop" variant="light">
          See the system
        </ArrowButton>
      </div>

      <a className="velion-hero__explore" href="#product-loop">
        <svg aria-hidden="true" viewBox="0 0 18 18">
          <path d="m3 6 6 6 6-6" />
        </svg>
        Scroll
      </a>
    </section>
  );
}
