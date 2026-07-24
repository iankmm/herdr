#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileReference {
    pub(crate) path: String,
    pub(crate) line: Option<u32>,
    pub(crate) column: Option<u32>,
}

pub(crate) fn parse_file_reference(candidate: &str) -> Option<FileReference> {
    let candidate = candidate.trim();
    if candidate.is_empty() || candidate.starts_with("http://") || candidate.starts_with("https://")
    {
        return None;
    }

    let (path, line, column) = split_location_suffix(candidate);
    if !looks_like_file_path(path) {
        return None;
    }

    Some(FileReference {
        path: path.to_string(),
        line,
        column,
    })
}

fn split_location_suffix(candidate: &str) -> (&str, Option<u32>, Option<u32>) {
    let Some((before_last, last)) = candidate.rsplit_once(':') else {
        return (candidate, None, None);
    };

    if let Some(column) = parse_positive_u32(last) {
        if let Some((path, line)) = before_last.rsplit_once(':') {
            if let Some(line) = parse_positive_u32(line) {
                return (path, Some(line), Some(column));
            }
        }
        return (before_last, Some(column), None);
    }

    if let Some(line) = parse_line_range_start(last) {
        return (before_last, Some(line), None);
    }

    (candidate, None, None)
}

fn parse_positive_u32(value: &str) -> Option<u32> {
    value.parse::<u32>().ok().filter(|number| *number > 0)
}

fn parse_line_range_start(value: &str) -> Option<u32> {
    let (start, end) = value.split_once('-')?;
    let start = parse_positive_u32(start)?;
    let end = parse_positive_u32(end)?;
    (end >= start).then_some(start)
}

fn looks_like_file_path(path: &str) -> bool {
    if path.is_empty() || path.ends_with(['/', '\\']) {
        return false;
    }

    if path.starts_with('/')
        || path.starts_with("./")
        || path.starts_with("../")
        || path.starts_with("~/")
        || path.contains('/')
        || path.contains('\\')
    {
        return true;
    }

    if path.starts_with('.') {
        return path.len() > 1;
    }

    let Some((stem, extension)) = path.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty()
        && !extension.is_empty()
        && extension
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '+'))
}

#[cfg(test)]
mod tests {
    use super::{parse_file_reference, FileReference};

    fn reference(path: &str, line: Option<u32>, column: Option<u32>) -> FileReference {
        FileReference {
            path: path.to_string(),
            line,
            column,
        }
    }

    #[test]
    fn parses_common_agent_file_references() {
        assert_eq!(
            parse_file_reference("README.md:44-50"),
            Some(reference("README.md", Some(44), None))
        );
        assert_eq!(
            parse_file_reference("./src/app/actions.rs:472:5"),
            Some(reference("./src/app/actions.rs", Some(472), Some(5)))
        );
        assert_eq!(
            parse_file_reference("infra/dev-dashboard/app/devbox/page.tsx"),
            Some(reference(
                "infra/dev-dashboard/app/devbox/page.tsx",
                None,
                None
            ))
        );
        assert_eq!(
            parse_file_reference(".env"),
            Some(reference(".env", None, None))
        );
    }

    #[test]
    fn rejects_non_file_tokens_and_invalid_locations() {
        assert_eq!(parse_file_reference("devbox"), None);
        assert_eq!(parse_file_reference("https://example.com/file.rs:4"), None);
        assert_eq!(parse_file_reference("src/"), None);
        assert_eq!(parse_file_reference("README.md:0"), None);
        assert_eq!(parse_file_reference("README.md:20-4"), None);
    }
}
