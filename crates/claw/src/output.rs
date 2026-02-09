/// Format a key-value pair for display.
pub fn kv(key: &str, value: &str) -> String {
    format!("{key:>16}: {value}")
}

/// Format a header line.
pub fn header(title: &str) -> String {
    format!("=== {title} ===")
}
