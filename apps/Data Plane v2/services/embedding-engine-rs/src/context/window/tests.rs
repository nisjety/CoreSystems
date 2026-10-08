use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use super::*;

/// Text with every width of UTF-8 in it, so a byte offset that lands inside a
/// character is hit by the very first slice that gets it wrong.
const ALPHABET: &[char] = &[
    'a', 'b', 'e', 'k', 's', 't', ' ', ' ', ' ', '.', '\n', 'æ', 'ø', 'å', 'Æ', 'Ø', 'Å', 'é', 'ß',
    '€', '日', '本', '😀', '🐟',
];

fn document(rng: &mut StdRng, chars: usize) -> String {
    (0..chars)
        .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())])
        .collect()
}

fn chars_of(text: &str) -> Vec<char> {
    text.chars().collect()
}

/// A chunk taken from character positions `[start, end)` of the document.
fn chunk_of(document: &str, start: usize, end: usize) -> String {
    chars_of(document)[start..end].iter().collect()
}

fn config() -> WindowConfig {
    WindowConfig::default()
}

#[test]
fn the_window_size_sets_the_overlap_and_the_header_as_designed() {
    let config = WindowConfig::default();
    assert_eq!(config.window_chars(), 32_000);
    assert_eq!(config.overlap_chars(), 4_000);
    assert_eq!(config.header_chars(), 2_000);
    assert_eq!(config.stride(), 28_000);
    for too_small in [0, 1, MIN_WINDOW_CHARS - 1] {
        let error = WindowConfig::new(too_small).expect_err("too small");
        assert!(
            error.contains("CONTEXTUAL_RETRIEVAL_WINDOW_CHARS"),
            "{error}"
        );
    }
    assert_eq!(
        WindowConfig::new(MIN_WINDOW_CHARS).unwrap().window_chars(),
        MIN_WINDOW_CHARS
    );
}

#[test]
fn a_document_that_fits_is_the_window_whole() {
    let mut rng = StdRng::seed_from_u64(1);
    for length in [0, 1, 500, 31_999, 32_000] {
        let text = document(&mut rng, length);
        let windowed = WindowedDocument::new(&text, config());
        assert!(windowed.is_whole(), "{length}");
        assert_eq!(windowed.total_chars(), length);
        let chunk = chunk_of(&text, 0, length.min(10));
        let window = windowed.window_for(&chunk, 0, Some(1));
        assert_eq!(window.kind, WindowKind::Whole);
        assert_eq!(window.id, 0);
        assert_eq!(window.text, text.as_str());
        assert!(
            matches!(window.text, Cow::Borrowed(_)),
            "a whole document is not copied"
        );
    }
}

/// A document one character over is cut: the boundary is exact, not approximate.
#[test]
fn a_document_one_character_over_is_cut_into_two_overlapping_sections() {
    let text = "ø".repeat(32_001);
    let windowed = WindowedDocument::new(&text, config());
    assert!(!windowed.is_whole());
    let ranges: Vec<(usize, usize)> = windowed.sections.iter().map(|s| (s.start, s.end)).collect();
    assert_eq!(ranges, vec![(0, 32_000), (28_000, 32_001)]);
}

#[test]
fn the_sections_cover_the_whole_document_and_overlap_by_exactly_the_overlap() {
    let mut rng = StdRng::seed_from_u64(2);
    for length in [32_001, 56_000, 60_001, 90_000, 123_457] {
        let text = document(&mut rng, length);
        let windowed = WindowedDocument::new(&text, config());
        let ranges: Vec<(usize, usize)> =
            windowed.sections.iter().map(|s| (s.start, s.end)).collect();
        assert_eq!(ranges[0].0, 0, "{length}");
        assert_eq!(ranges.last().unwrap().1, length, "{length}");
        for pair in ranges.windows(2) {
            let ((_, previous_end), (next_start, _)) = (pair[0], pair[1]);
            assert_eq!(
                previous_end - next_start,
                config().overlap_chars(),
                "{length}: {pair:?}"
            );
        }
        for (start, end) in &ranges {
            assert!(end - start <= config().window_chars(), "{length}");
        }
        // Every character is in some section.
        let mut covered = vec![false; length];
        for (start, end) in &ranges {
            covered[*start..*end].fill(true);
        }
        assert!(covered.iter().all(|c| *c), "{length}");
    }
}

/// The central property, over many random documents and chunks: the window always
/// contains the chunk it was chosen for, it is exactly what its section says, and
/// it never carries more than a window, a header and two markers.
#[test]
fn every_chunk_is_read_inside_a_window_that_contains_it() {
    let mut rng = StdRng::seed_from_u64(3);
    for round in 0..12 {
        let length = rng.gen_range(32_001..130_000);
        let text = document(&mut rng, length);
        let windowed = WindowedDocument::new(&text, config());
        let bound = config().window_chars() + config().header_chars() + 4 * ELIDED.len() + 8;
        for _ in 0..40 {
            let len = rng.gen_range(20..1_500);
            let start = rng.gen_range(0..length - len);
            let chunk = chunk_of(&text, start, start + len);
            let window = windowed.window_for(&chunk, 0, None);
            assert_eq!(window.kind, WindowKind::Section);
            assert!(
                window.text.contains(&chunk),
                "round {round}: the window for chunk [{start},{}) does not contain it",
                start + len
            );
            assert!(window.text.chars().count() <= bound, "round {round}");
        }
    }
}

/// "Farthest from an edge", checked against a brute-force scorer on every chunk
/// position of a document around both cuts, including the ones that straddle.
#[test]
fn a_chunk_is_read_in_the_section_where_it_sits_farthest_from_a_cut_edge() {
    let text = "x".repeat(70_000);
    let windowed = WindowedDocument::new(&text, config());
    let sections: Vec<(usize, usize)> =
        windowed.sections.iter().map(|s| (s.start, s.end)).collect();
    let total = 70_000usize;
    for start in (0..total - 900).step_by(137) {
        let chunk = "x".repeat(900);
        let located = Located { start, len: 900 };
        let chosen = windowed.section_for(&located);
        let score = |(s, e): (usize, usize)| -> i64 {
            let left = if s == 0 {
                i64::MAX
            } else {
                start as i64 - s as i64
            };
            let right = if e == total {
                i64::MAX
            } else {
                e as i64 - (start + 900) as i64
            };
            left.min(right)
        };
        let best = sections.iter().map(|range| score(*range)).max().unwrap();
        assert_eq!(score(sections[chosen]), best, "chunk at {start}");
        // And the chunk does not straddle a cut of the chosen section when any
        // section holds it whole.
        if best >= 0 {
            assert!(score(sections[chosen]) >= 0, "chunk at {start}");
        }
        let _ = chunk;
    }
}

/// A chunk in the overlap goes to the section it is farther inside: the middle of
/// the overlap is a tie and the earlier section wins, deterministically.
#[test]
fn a_tie_between_two_sections_goes_to_the_earlier_one() {
    let text = "x".repeat(70_000);
    let windowed = WindowedDocument::new(&text, config());
    // Section 0 ends at 32,000 and section 1 starts at 28,000. A 100-character chunk
    // centred in the overlap is 1,950 from each edge.
    let located = Located {
        start: 29_950,
        len: 100,
    };
    assert_eq!(windowed.section_for(&located), 0);
    // One character later it is closer to section 0's end, so section 1 wins.
    let located = Located {
        start: 29_951,
        len: 100,
    };
    assert_eq!(windowed.section_for(&located), 1);
}

#[test]
fn chunks_of_one_section_get_the_same_text_and_that_text_is_a_stable_prefix() {
    let mut rng = StdRng::seed_from_u64(4);
    let text = document(&mut rng, 100_000);
    let windowed = WindowedDocument::new(&text, config());
    let a = chunk_of(&text, 41_000, 41_800);
    let b = chunk_of(&text, 47_000, 47_800);
    let (wa, wb) = (
        windowed.window_for(&a, 0, None),
        windowed.window_for(&b, 1, None),
    );
    assert_eq!(wa.id, wb.id, "both chunks sit deep inside the same section");
    assert_eq!(wa.text, wb.text);
    // Built once, then shared: the same allocation, not an equal copy.
    assert!(std::ptr::eq(wa.text.as_ptr(), wb.text.as_ptr()));
}

#[test]
fn a_section_carries_the_documents_start_unless_it_already_does() {
    let mut rng = StdRng::seed_from_u64(5);
    let text = document(&mut rng, 100_000);
    let windowed = WindowedDocument::new(&text, config());
    let header = chunk_of(&text, 0, config().header_chars());
    let first = windowed.window_for(&chunk_of(&text, 1_000, 1_500), 0, None);
    assert_eq!(first.id, 0);
    assert!(
        first.text.starts_with(&header),
        "the first section begins with the document"
    );
    assert!(
        !first.text.starts_with(&format!("{header}\n\n{ELIDED}")),
        "and does not repeat it"
    );
    let later = windowed.window_for(&chunk_of(&text, 50_000, 50_800), 0, None);
    assert!(later.id > 0);
    assert!(
        later.text.starts_with(&format!("{header}\n\n{ELIDED}\n\n")),
        "a later section opens with the header and says text was left out"
    );
    // The last section reaches the end of the document and says nothing is missing
    // after it; every other one says there is.
    let last = windowed.window_for(&chunk_of(&text, 99_000, 99_800), 0, None);
    assert_eq!(last.id, windowed.sections.len() - 1);
    assert!(!last.text.ends_with(ELIDED));
    assert!(first.text.ends_with(ELIDED));
}

/// Every offset the module slices at comes from `char_indices`, so a document made of
/// characters of every width, with chunks that start and end on those characters,
/// never panics. Run over a size that spans several sections and the grid.
#[test]
fn windowing_never_splits_a_character() {
    let text: String = "Røyking av laks 🐟 i Tromsø koster 5 € per kilo, 日本語も。".repeat(2_500);
    let windowed = WindowedDocument::new(&text, config());
    assert!(windowed.total_chars() > 100_000);
    let chars = chars_of(&text);
    for start in (0..chars.len() - 60).step_by(997) {
        let chunk: String = chars[start..start + 60].iter().collect();
        let window = windowed.window_for(&chunk, 0, None);
        assert!(window.text.contains(&chunk), "chunk at {start}");
        assert!(windowed.shrunk_for(&chunk, 0, None).is_some());
    }
}

/// The sparse grid against the naive answer, at every kind of position.
#[test]
fn the_character_to_byte_mapping_agrees_with_the_obvious_one() {
    let mut rng = StdRng::seed_from_u64(6);
    let text = document(&mut rng, 5_000);
    let windowed = WindowedDocument::new(&text, config());
    let naive: Vec<usize> = text.char_indices().map(|(byte, _)| byte).collect();
    for (index, byte) in naive.iter().enumerate() {
        assert_eq!(windowed.byte_of_char(index), *byte, "char {index}");
        assert_eq!(windowed.char_of_byte(*byte), index, "byte {byte}");
    }
    assert_eq!(windowed.byte_of_char(naive.len()), text.len());
    assert_eq!(windowed.byte_of_char(naive.len() + 10), text.len());
    // The memory cost is a few entries, not one per character.
    assert!(windowed.grid.len() <= naive.len() / GRID + 2);
}

#[test]
fn an_empty_document_is_handled() {
    let windowed = WindowedDocument::new("", config());
    assert!(windowed.is_whole());
    assert_eq!(windowed.total_chars(), 0);
    assert_eq!(windowed.byte_of_char(0), 0);
    assert_eq!(windowed.window_for("anything", 3, Some(5)).text, "");
    assert!(windowed.shrunk_for("anything", 3, Some(5)).is_none());
}

/// A chunk whose text the chunker changed (whitespace collapsed, a ligature
/// expanded) cannot be found. It then goes by its position in the document, which is
/// deterministic: the same inputs always read the same window.
#[test]
fn a_chunk_that_cannot_be_found_is_placed_by_its_index_deterministically() {
    let mut rng = StdRng::seed_from_u64(7);
    let text = document(&mut rng, 100_000);
    let windowed = WindowedDocument::new(&text, config());
    let altered = "this text is not in the document at all, not even once";
    assert!(!text.contains(altered));
    // With the document's chunk count known the position is proportional.
    let first = windowed.window_for(altered, 0, Some(100));
    let middle = windowed.window_for(altered, 50, Some(100));
    let last = windowed.window_for(altered, 99, Some(100));
    assert_eq!(first.id, 0);
    assert!(
        middle.id > 0 && middle.id < last.id,
        "{} {} {}",
        first.id,
        middle.id,
        last.id
    );
    assert_eq!(last.id, windowed.sections.len() - 1);
    assert_eq!(
        windowed.window_for(altered, 50, Some(100)).text,
        middle.text
    );
    // Without it the index is scaled by an assumed chunk size, still deterministic.
    let a = windowed.window_for(altered, 70, None);
    let b = windowed.window_for(altered, 70, None);
    assert_eq!((a.id, a.text.as_ref()), (b.id, b.text.as_ref()));
    // A negative index (a defect upstream) is treated as the start, not a panic.
    assert_eq!(windowed.window_for(altered, -4, Some(100)).id, 0);
}

#[test]
fn a_chunk_that_differs_only_by_surrounding_whitespace_is_still_found() {
    let mut rng = StdRng::seed_from_u64(8);
    let text = document(&mut rng, 80_000);
    let windowed = WindowedDocument::new(&text, config());
    let core = chunk_of(&text, 60_000, 60_500);
    let padded = format!("\n  {core}  \n");
    assert_eq!(
        windowed.window_for(&padded, 0, None).id,
        windowed.window_for(&core, 0, None).id
    );
    assert!(windowed
        .window_for(&padded, 0, None)
        .text
        .contains(core.trim()));
}

/// Boilerplate repeats. The occurrence nearest where the chunk should be is the
/// one meant; without a chunk count there is nothing to be near and the first wins.
#[test]
fn a_repeated_chunk_is_placed_at_the_occurrence_nearest_its_position() {
    let boilerplate = "Dette dokumentet er konfidensielt og tilhører Fjordfisk AS.";
    let mut text = String::new();
    let filler = "ø".repeat(30_000);
    for _ in 0..4 {
        text.push_str(boilerplate);
        text.push_str(&filler);
    }
    let windowed = WindowedDocument::new(&text, config());
    assert!(!windowed.is_whole());
    let near_start = windowed.window_for(boilerplate, 0, Some(4));
    let near_end = windowed.window_for(boilerplate, 3, Some(4));
    // The fourth occurrence starts three quarters of the way in; the section read
    // for it is the one that holds that occurrence, and the first is read in the first.
    let fourth = 3 * windowed.total_chars() / 4;
    let (start, end) = (
        windowed.sections[near_end.id].start,
        windowed.sections[near_end.id].end,
    );
    assert!(
        start <= fourth && fourth + boilerplate.chars().count() <= end,
        "section {start}..{end}"
    );
    assert_eq!(near_start.id, 0);
    assert!(near_end.id > near_start.id);
    assert_eq!(
        windowed.window_for(boilerplate, 3, None).id,
        0,
        "no estimate: the first occurrence"
    );
}

/// A chunk this repetitive carries no position. It must not take a search through a
/// million occurrences, and must still land somewhere valid.
#[test]
fn a_chunk_with_endless_occurrences_is_bounded() {
    let text = "ab".repeat(60_000);
    let windowed = WindowedDocument::new(&text, config());
    let window = windowed.window_for("ab", 0, Some(1_000));
    assert!(window.text.contains("ab"));
}

#[test]
fn a_shrunk_window_is_half_the_size_centred_on_the_chunk() {
    let mut rng = StdRng::seed_from_u64(9);
    let text = document(&mut rng, 100_000);
    let windowed = WindowedDocument::new(&text, config());
    let chunk = chunk_of(&text, 50_000, 50_600);
    let shrunk = windowed
        .shrunk_for(&chunk, 0, None)
        .expect("a long document can shrink");
    assert_eq!(shrunk.kind, WindowKind::Shrunk);
    assert!(shrunk.text.contains(&chunk));
    let bound = config().window_chars() / 2 + 2 * (ELIDED.len() + 2);
    assert!(shrunk.text.chars().count() <= bound);
    assert!(shrunk.text.starts_with(ELIDED) && shrunk.text.ends_with(ELIDED));
    // Near the start of the document the window starts at it and says nothing is
    // missing before; likewise at the end.
    let head = windowed
        .shrunk_for(&chunk_of(&text, 10, 300), 0, None)
        .unwrap();
    assert!(!head.text.starts_with(ELIDED));
    let tail = windowed
        .shrunk_for(&chunk_of(&text, 99_500, 99_900), 0, None)
        .unwrap();
    assert!(!tail.text.ends_with(ELIDED));
}

/// A document that is already no larger than the shrunk window has nothing left to
/// give up: the caller must not retry the same call.
#[test]
fn a_document_already_small_cannot_shrink() {
    let text = "ø".repeat(16_000);
    let windowed = WindowedDocument::new(&text, config());
    assert!(windowed.shrunk_for("ø", 0, None).is_none());
    let text = "ø".repeat(16_001);
    let windowed = WindowedDocument::new(&text, config());
    assert!(windowed.shrunk_for("ø", 0, None).is_some());
}

/// A multi-megabyte document costs a few kilobytes of index.
#[test]
fn a_large_document_is_indexed_in_a_few_kilobytes() {
    let text = "Røyking av laks i Tromsø. ".repeat(200_000);
    let windowed = WindowedDocument::new(&text, config());
    assert!(windowed.total_chars() > 5_000_000);
    assert!(windowed.grid.len() * std::mem::size_of::<usize>() < 200_000);
    assert!(windowed.sections.len() > 150);
    let chunk = "Tromsø. Røyking";
    let window = windowed.window_for(chunk, 0, None);
    assert!(window.text.contains(chunk));
}
