pub(crate) fn single_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn shell_quote_display(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '/' | '.' | ':'))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::{shell_quote_display, single_line};

    #[test]
    fn collapses_whitespace_for_single_line_output() {
        assert_eq!(single_line("one\n  two\tthree"), "one two three");
    }

    #[test]
    fn quotes_shell_display_only_when_needed() {
        assert_eq!(shell_quote_display("/tmp/shea-file"), "/tmp/shea-file");
        assert_eq!(shell_quote_display("two words"), "'two words'");
        assert_eq!(shell_quote_display("it's"), "'it'\\''s'");
    }
}
