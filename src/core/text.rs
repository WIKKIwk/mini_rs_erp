pub(crate) fn trim_owned(mut value: String) -> String {
    let start = value.len() - value.trim_start().len();
    let trimmed_len = value.trim().len();
    value.truncate(start + trimmed_len);
    if start > 0 {
        value.drain(..start);
    }
    value
}

pub(crate) fn lowercase_ascii_owned(value: String) -> String {
    let mut value = trim_owned(value);
    value.make_ascii_lowercase();
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_trim_handles_empty_whitespace_and_unicode_boundaries() {
        assert_eq!(trim_owned(String::new()), "");
        assert_eq!(trim_owned("   ".to_string()), "");
        assert_eq!(trim_owned("\u{2003} value \u{2003}".to_string()), "value");
        assert_eq!(trim_owned("  two words  ".to_string()), "two words");
    }

    #[test]
    fn owned_ascii_lowercase_trims_without_changing_non_ascii_text() {
        assert_eq!(
            lowercase_ascii_owned("  IMAGE/JPEG  ".to_string()),
            "image/jpeg"
        );
        assert_eq!(lowercase_ascii_owned("  O‘ZBEK  ".to_string()), "o‘zbek");
    }
}
