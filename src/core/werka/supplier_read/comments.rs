pub(super) fn dispatch_record_needs_comment_scan(
    record: &crate::core::werka::models::DispatchRecord,
) -> bool {
    matches!(record.status.as_str(), "partial" | "rejected" | "cancelled")
        || !record.note.trim().is_empty()
}

pub(super) fn is_supplier_acknowledgment_comment(content: &str) -> bool {
    let (author, body) = parse_notification_comment(content);
    author.starts_with("Supplier") && body.trim().to_lowercase().starts_with("tasdiqlayman")
}

fn parse_notification_comment(content: &str) -> (String, String) {
    let trimmed = sanitize_notification_comment(content);
    if trimmed.is_empty() {
        return (String::new(), String::new());
    }
    let lines = trimmed.lines().collect::<Vec<_>>();
    if lines.len() >= 2 {
        let head = lines[0].trim();
        let body = lines[1..].join("\n").trim().to_string();
        if !body.is_empty()
            && ["Supplier", "Werka", "Customer", "Admin"]
                .iter()
                .any(|prefix| head.starts_with(prefix))
        {
            return (head.to_string(), body);
        }
    }
    ("Tizim".to_string(), trimmed)
}

fn sanitize_notification_comment(content: &str) -> String {
    content
        .trim()
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("\r\n", "\n")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}
