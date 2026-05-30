import { Composition } from 'remotion'

import { ProductReveal } from './ProductReveal'

export function RemotionRoot() {
  return (
    <Composition
      id="ProductReveal"
      component={ProductReveal}
      durationInFrames={240}
      fps={60}
      width={1920}
      height={1080}
    />
  )
}
