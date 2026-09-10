import type { CaptureRegion, RemoteVideoFrame } from '../types/public.js';

/**
 * EIERSKAP TIL FRAMES (viktig, lett å gjøre feil):
 *
 * `VideoFrame` og `ImageBitmap` holder GPU-/dekoderressurser som må frigjøres
 * eksplisitt — de forsvinner ikke med søppelrydding. Regelen i remote-core er:
 *
 *  - Den som PRODUSERER en frame eier den gjennom hele utsendelsen og
 *    frigjør den etterpå (RemoteSession gjør dette etter at alle abonnenter
 *    har fått se den).
 *  - En abonnent som bare bruker framen synkront (f.eks. CanvasRenderer som
 *    tegner den) skal IKKE frigjøre den — da ville neste abonnent fått en
 *    lukket frame.
 *  - En abonnent som trenger framen ETTER at handleren returnerer (AI-laget,
 *    som sender den videre asynkront) må få sin EGEN kopi via
 *    `cloneVideoFrame`, og eier da ansvaret for å frigjøre den.
 */
export function releaseVideoFrame(frame: RemoteVideoFrame): void {
  frame.videoFrame?.close();
  frame.bitmap?.close();
}

/**
 * Lager en kopi som mottakeren eier. Billig: WebCodecs sin `clone()` deler
 * den underliggende bufferen og teller bare referanser.
 *
 * MERK — kun `videoFrame` er en reell, uavhengig kopi. `bitmap` er samme
 * objekt-referanse som originalen (`ImageBitmap` har ingen `clone()`; en ekte
 * kopi krever asynkron `createImageBitmap()`, som ikke passer denne synkrone
 * signaturen). En mottaker som beholder en bitmap-basert frame etter at
 * produsenten har kalt `releaseVideoFrame()` på originalen, får altså en
 * LUKKET bitmap — ikke trygt å tegne fra. Ufarlig i dag: RustDeskProtocol
 * produserer aldri bitmap-baserte frames (kun `videoFrame`), så ingen
 * nåværende kode rammes — men en fremtidig protokollimplementasjon som gjør
 * det, må ikke stole på denne funksjonen for bitmaps. CanvasRenderer unngår
 * problemet ved å eie sin egen tegnebuffer i stedet for å beholde frame-
 * objektet på tvers av hendelser — se renderer/CanvasRenderer.ts.
 */
export function cloneVideoFrame(frame: RemoteVideoFrame): RemoteVideoFrame {
  return {
    ...frame,
    videoFrame: frame.videoFrame?.clone(),
    bitmap: frame.bitmap,
  };
}

/**
 * Beskjærer en frame til et delområde. Krever en WebCodecs-`VideoFrame` som
 * kilde — `visibleRect` gjør beskjæringen uten å kopiere pikslene.
 * Returnerer en NY frame som kalleren eier.
 */
export function cropVideoFrame(frame: RemoteVideoFrame, region: CaptureRegion): RemoteVideoFrame {
  const source = frame.videoFrame;
  if (!source) {
    throw new Error('cropVideoFrame requires a frame backed by a WebCodecs VideoFrame');
  }

  // Klem området til framens faktiske grenser — en visibleRect utenfor
  // bildet gir en hard konstruktørfeil i stedet for et tomt resultat.
  const x = Math.max(0, Math.min(region.x, frame.width));
  const y = Math.max(0, Math.min(region.y, frame.height));
  const width = Math.max(1, Math.min(region.width, frame.width - x));
  const height = Math.max(1, Math.min(region.height, frame.height - y));

  const cropped = new VideoFrame(source, { visibleRect: { x, y, width, height } });
  return {
    width: cropped.displayWidth,
    height: cropped.displayHeight,
    timestamp: frame.timestamp,
    displayId: frame.displayId,
    videoFrame: cropped,
  };
}
