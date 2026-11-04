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

## Manifest path

For several slides, write a manifest and let the builder lay them out and check each slide.

- Write `manifest.json` in the working directory with `version` 1, `title`, `locale` (for example "nb-NO") and `slides`. Each slide has an `id`, a `layout` (title, title_bullets, title_only, section, two_column with `left` and `right`, or image with `image`: a path or up to four) and its text.
- Run `python3 /opt/verevon/skills/office/pptx/scripts/build_deck.py manifest.json presentasjon.pptx` with subprocess and print its output. It writes the deck and prints one JSON line with a state per slide: ok, warn or error.
- Fix each error slide, run the builder again and read the new report. Shorten or split a warn slide whose text does not fit.
- Limits: 40 slides, 5 bullets per slide (the builder warns above 6), 90 characters per bullet, 120 per title, 2,000 characters of notes per slide. A number in a bullet needs its source in notes. Picture paths are relative: no `..` and no leading slash.
- Use python-pptx by hand only for a layout the builder does not have.
- For a deck from a report in this chat, use `deck` `create_from_artifact` when offered.
