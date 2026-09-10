import type { RemoteVideoFrame } from '../types/public.js';
import type { RdVideoCodec, RdVideoFrame } from '../protocol/rustdesk/messages/types.js';
import type { Logger } from '../logging/Logger.js';
import { noopLogger } from '../logging/Logger.js';
import { CodecError } from '../errors/RemoteError.js';

/**
 * WebCodecs-strenger per RustDesk-kodek. VP8/VP9/AV1 er trygge valg: de har
 * en software-dekoder i alle nettlesere som støtter WebCodecs i det hele
 * tatt. H264/H265 er merket usikre fordi RustDesk KUN koder dem med
 * maskinvare (ingen software-fallback finnes i prosjektet), og
 * nettleserstøtten varierer med GPU/plattform — se
 * docs/rustdesk-protocol.md, "Video codec and display handling".
 */
const CODEC_STRINGS: Readonly<Record<RdVideoCodec, string>> = {
  vp8: 'vp8',
  vp9: 'vp09.00.10.08',
  av1: 'av01.0.04M.08',
  h264: 'avc1.42E01E',
  h265: 'hvc1.1.6.L93.B0',
};

export interface MediaCapabilities {
  readonly videoCodecs: readonly RdVideoCodec[];
  readonly webCodecsAvailable: boolean;
}

/**
 * Spør nettleseren hvilke av RustDesk sine kodeker den faktisk kan dekode,
 * i stedet for å anta. Resultatet er det vi annonserer i
 * `SupportedDecoding` under innlogging.
 */
export async function detectMediaCapabilities(): Promise<MediaCapabilities> {
  if (typeof VideoDecoder === 'undefined') {
    return { videoCodecs: [], webCodecsAvailable: false };
  }

  const supported: RdVideoCodec[] = [];
  for (const [codec, codecString] of Object.entries(CODEC_STRINGS) as Array<[RdVideoCodec, string]>) {
    try {
      const result = await VideoDecoder.isConfigSupported({ codec: codecString });
      if (result.supported === true) supported.push(codec);
    } catch {
      // isConfigSupported kaster på ugyldige/ukjente strenger i noen
      // nettlesere i stedet for å svare {supported:false} — behandle det
      // som "ikke støttet" og gå videre.
    }
  }
  return { videoCodecs: supported, webCodecsAvailable: true };
}

export interface RemoteVideoDecoderOptions {
  readonly onFrame: (frame: RemoteVideoFrame) => void;
  readonly onError?: (error: CodecError) => void;
  readonly logger?: Logger;
}

/**
 * Dekoder RustDesk sine kodede videopakker til WebCodecs-`VideoFrame`-objekter.
 *
 * To ting her er reelle krav, ikke pynt:
 *  - Delta-frames FØR første keyframe må forkastes. WebCodecs kan ikke dekode
 *    dem, og å sende dem inn gir en hard dekoderfeil.
 *  - Kodeken kan bytte midt i økten (RustDesk reforhandler når settet av
 *    tilkoblede klienter endres). Vi rekonfigurerer da, men først ved neste
 *    keyframe — en rekonfigurering midt i en GOP gir ødelagt bilde.
 */
export class RemoteVideoDecoder {
  private decoder: VideoDecoder | undefined;
  private configuredCodec: RdVideoCodec | undefined;
  private sawKeyframe = false;
  private readonly logger: Logger;
  private closed = false;
  private currentDisplay = 0;

  constructor(private readonly options: RemoteVideoDecoderOptions) {
    this.logger = options.logger ?? noopLogger;
  }

  /** Antall frames forkastet fordi de kom før første keyframe. */
  droppedBeforeKeyframe = 0;

  decode(videoFrame: RdVideoFrame): void {
    if (this.closed) return;
    if (typeof VideoDecoder === 'undefined') {
      this.fail(new CodecError('WebCodecs VideoDecoder is not available in this browser'));
      return;
    }

    this.currentDisplay = videoFrame.display;
    const hasKeyframe = videoFrame.frames.some((frame) => frame.key);

    if (this.configuredCodec !== videoFrame.codec) {
      if (!hasKeyframe) {
        // Kan ikke bytte kodek uten et keyframe å starte på.
        this.droppedBeforeKeyframe += videoFrame.frames.length;
        return;
      }
      this.configure(videoFrame.codec);
    }

    const decoder = this.decoder;
    if (!decoder) return;

    for (const chunk of videoFrame.frames) {
      if (!this.sawKeyframe && !chunk.key) {
        this.droppedBeforeKeyframe += 1;
        continue;
      }
      if (chunk.key) this.sawKeyframe = true;

      try {
        decoder.decode(
          new EncodedVideoChunk({
            type: chunk.key ? 'key' : 'delta',
            // pts er int64 i protokollen; WebCodecs vil ha mikrosekunder som number.
            timestamp: Number(chunk.pts),
            data: chunk.data,
          }),
        );
      } catch (error) {
        this.fail(
          new CodecError(
            `Failed to decode a ${videoFrame.codec} chunk: ${error instanceof Error ? error.message : String(error)}`,
            { codec: videoFrame.codec },
          ),
        );
        return;
      }
    }
  }

  private configure(codec: RdVideoCodec): void {
    this.reset();

    const codecString = CODEC_STRINGS[codec];
    const decoder = new VideoDecoder({
      output: (frame) => {
        this.options.onFrame({
          width: frame.displayWidth,
          height: frame.displayHeight,
          timestamp: frame.timestamp ?? 0,
          displayId: String(this.currentDisplay),
          videoFrame: frame,
        });
      },
      error: (error) => {
        this.fail(new CodecError(`Video decoder error: ${error.message}`, { codec }));
      },
    });

    // Ingen `description`: RustDesk sender H264/H265 i Annex-B-form fra
    // hwcodec-pipelinen, som WebCodecs tar imot uten avcC-header.
    decoder.configure({ codec: codecString, optimizeForLatency: true });

    this.decoder = decoder;
    this.configuredCodec = codec;
    this.sawKeyframe = false;
    this.logger.debug('Configured video decoder', { codec, codecString });
  }

  /**
   * En fatal WebCodecs-feil LUKKER dekoderen (per spesifikasjonen). Lot vi den
   * stå igjen installert, ville hvert påfølgende `decode()` kastet
   * InvalidStateError i det uendelige: videoen ville vært død for godt mens
   * økten fortsatt meldte 'connected'. Vi nullstiller derfor, slik at neste
   * keyframe konfigurerer en frisk dekoder — og rapporterer feilen oppover.
   */
  private fail(error: CodecError): void {
    this.logger.error(error.message);
    if (this.decoder && this.decoder.state === 'closed') {
      this.reset();
    }
    this.options.onError?.(error);
  }

  /**
   * Kalles når strømmen bytter kilde (skjermbytte) uten at kodeken endres.
   * Referansebildene i dekoderen tilhører den FORRIGE skjermen, så delta-
   * frames fra den nye GOP-en må forkastes til neste keyframe. Vi river ikke
   * ned selve dekoderen — bare kravet om et keyframe kommer tilbake.
   */
  resetForNewStream(): void {
    this.sawKeyframe = false;
  }

  private reset(): void {
    if (this.decoder && this.decoder.state !== 'closed') {
      this.decoder.close();
    }
    this.decoder = undefined;
    this.configuredCodec = undefined;
    this.sawKeyframe = false;
  }

  close(): void {
    this.closed = true;
    this.reset();
  }
}
