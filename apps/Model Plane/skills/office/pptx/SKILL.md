---
name: PowerPoint (pptx)
description: Build a PowerPoint .pptx deck with python-pptx in code_interpreter: one message per slide, a 16:9 layout, native or image charts, and the checks before delivery.
---

# PowerPoint decks (.pptx)

Use this for a presentation, slides or a deck the user will open in PowerPoint.

## Build

- Use python-pptx 1.0.2. Set a 16:9 canvas (13.333 x 7.5 inches) and build from the default template layouts: Title Slide, Title and Content, Title Only and Blank.
- One message per slide: a headline that states the point, then at most five short bullets. Keep body text at 18 pt or larger.
- Write the deck in the language the user wrote in. Use the figures and names the user gave; label any assumption on the slide.
- Charts: a native chart from CategoryChartData, or a matplotlib PNG (plt.savefig, never plt.show) added with add_picture.
- Keep every shape inside the slide, and check text length against each box.
- Speaker notes are optional. Use them to record the source or the calculation behind a number.

## Deliver

- Save the final file at the top level of the working directory, for example "presentasjon.pptx".
- Reopen it with Presentation(path) and print the slide count and each slide title, to confirm it reads back.
- When the code_interpreter result has an office_checks line for a file, that line is the file's structural check. If it says FAILED, regenerate the file, fix the reported problem and run it again before you call it ready. Describe a file as structurally checked only when such a line says so, and never as verified in PowerPoint.
