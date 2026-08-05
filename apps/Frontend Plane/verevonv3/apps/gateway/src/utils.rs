pub(crate) fn trim_opt(value: Option<String>) -> Option<String> {
    value.and_then(|item| empty_to_none(&item))
}

pub(crate) fn empty_to_none(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

pub(crate) fn slugify(input: &str) -> String {
    input
        .trim()
        .to_lowercase()
        .replace('æ', "ae")
        .replace('ø', "o")
        .replace('å', "a")
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}
