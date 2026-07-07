import { Composition } from 'remotion'

import { ProductReveal } from './ProductReveal'
import { VelionSignalDroneShort } from './VelionSignalDroneShort'

export function RemotionRoot() {
  return (
    <>
      <Composition
        id="ProductReveal"
        component={ProductReveal}
        durationInFrames={240}
        fps={60}
        width={1920}
        height={1080}
      />
      <Composition
        id="VelionSignalDroneShort"
        component={VelionSignalDroneShort}
        durationInFrames={351}
        fps={25}
        width={1920}
        height={1080}
      />
    </>
  )
}
