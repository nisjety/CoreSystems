# Reference artwork

Sphere artwork and grain texture observed in the user-selected ElevenLabs
marketing carousel on 2026-10-25. Kept local so rendering Pulse makes no
requests to the reference site's CDN.

Base URL: `https://eleven-public-cdn.elevenlabs.io/marketing_website/_next/static/media/`

| Local image | Reference filename |
| --- | --- |
| orange.png | creative-1.be8a6b68.png |
| purple.png | creative-5.f21107e1.png |
| dark.png | creative-4.60676b95.png |
| sage.png | creative-9.e00adc24.png |
| cream.png | creative-8.2ab723e3.png |
| silver.png | creative-3.04cea590.png |
| noise.png | noise.24b8225d.png |

The implementation uses original image files inside native circular masks;
the page layout, navigation, text and report actions remain Solid components.

`inter-regular.ttf` is Inter Regular from Google Fonts' public font stylesheet
(`https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600&display=swap`).
Its SIL Open Font License is retained in `INTER-LICENSE.txt`.

`microsoft-reference.png` is the Microsoft orb image supplied by the user on
26 October 2026. It is served locally, unchanged, and framed with CSS inside
the connector circles. Microsoft Clarity keeps its original appearance; other
connectors apply their palette through a CSS color-blend layer, preserving the
same ribbed surface and highlights.
