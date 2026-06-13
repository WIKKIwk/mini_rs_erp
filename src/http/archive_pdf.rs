use crate::core::werka::models::{DispatchRecord, WerkaArchiveResponse};

const LINES_PER_PAGE: usize = 46;

pub(crate) fn build_archive_pdf(data: &WerkaArchiveResponse) -> Vec<u8> {
    let lines = archive_lines(data);
    let page_count = lines.len().div_ceil(LINES_PER_PAGE).max(1);
    let object_count = 3 + page_count * 2;
    let mut objects = vec![String::new(); object_count + 1];
    let mut kids = Vec::with_capacity(page_count);

    objects[1] = "<< /Type /Catalog /Pages 2 0 R >>".to_string();
    objects[3] = "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".to_string();

    for page in 0..page_count {
        let page_id = 4 + page * 2;
        let content_id = page_id + 1;
        let start = page * LINES_PER_PAGE;
        let end = (start + LINES_PER_PAGE).min(lines.len());
        let stream = build_pdf_text_stream(&lines[start..end]);
        objects[page_id] = format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 842 595] /Resources << /Font << /F1 3 0 R >> >> /Contents {content_id} 0 R >>"
        );
        objects[content_id] = format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            stream.len(),
            stream
        );
        kids.push(format!("{page_id} 0 R"));
    }

    objects[2] = format!(
        "<< /Type /Pages /Kids [{}] /Count {} >>",
        kids.join(" "),
        page_count
    );

    let mut out = String::from("%PDF-1.4\n");
    let mut offsets = vec![0_usize; object_count + 1];
    for id in 1..=object_count {
        offsets[id] = out.len();
        out.push_str(&format!("{id} 0 obj\n{}\nendobj\n", objects[id]));
    }

    let xref_offset = out.len();
    out.push_str(&format!("xref\n0 {}\n", object_count + 1));
    out.push_str("0000000000 65535 f \n");
    for offset in offsets.iter().take(object_count + 1).skip(1) {
        out.push_str(&format!("{offset:010} 00000 n \n"));
    }
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
        object_count + 1,
        xref_offset
    ));

    out.into_bytes()
}

fn archive_lines(data: &WerkaArchiveResponse) -> Vec<String> {
    let mut lines = vec![
        "Werka Archive Report".to_string(),
        format!("Kind: {}", data.kind),
        format!("Period: {}", data.period),
        format!("From: {}", empty_dash(&data.from)),
        format!("To: {}", empty_dash(&data.to)),
        format!("Records: {}", data.summary.record_count),
    ];

    if !data.summary.totals_by_uom.is_empty() {
        lines.push("Totals:".to_string());
        for total in &data.summary.totals_by_uom {
            lines.push(format!("  {}: {}", total.uom, format_go_4g(total.qty)));
        }
    }

    lines.push(String::new());
    lines.push(
        "Date                 Type              Party                         Item                 Sent        Accepted    Status"
            .to_string(),
    );
    for item in &data.items {
        lines.push(format_dispatch_line(item));
    }

    lines
}

fn format_dispatch_line(item: &DispatchRecord) -> String {
    format!(
        "{:<19}  {:<16}  {:<28}  {:<20}  {:>10}  {:>10}  {}",
        item.created_label,
        item.record_type,
        item.supplier_name,
        item.item_code,
        format_go_4g(item.sent_qty),
        format_go_4g(item.accepted_qty),
        item.status,
    )
}

fn build_pdf_text_stream(lines: &[String]) -> String {
    let mut stream = String::from("BT\n/F1 8 Tf\n36 555 Td\n11 TL\n");
    for line in lines {
        stream.push('(');
        stream.push_str(&escape_pdf_text(line));
        stream.push_str(") Tj\nT*\n");
    }
    stream.push_str("ET");
    stream
}

fn escape_pdf_text(value: &str) -> String {
    let mut escaped = String::new();
    for ch in value.trim().chars().take(132) {
        match ch {
            '\\' | '(' | ')' => {
                escaped.push('\\');
                escaped.push(ch);
            }
            '\n' | '\r' | '\t' => escaped.push(' '),
            ch if ch.is_ascii() && !ch.is_control() => escaped.push(ch),
            _ => escaped.push('?'),
        }
    }
    escaped
}

fn empty_dash(value: &str) -> &str {
    let trimmed = value.trim();
    if trimmed.is_empty() { "-" } else { trimmed }
}

fn format_go_4g(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }

    let exponent = value.abs().log10().floor() as i32;
    if !(-4..4).contains(&exponent) {
        let mantissa = value / 10_f64.powi(exponent);
        return format!(
            "{}e{}{abs_exponent:02}",
            trim_float(format!("{mantissa:.3}")),
            if exponent < 0 { "-" } else { "+" },
            abs_exponent = exponent.abs()
        );
    }

    let decimals = if exponent >= 0 {
        3_usize.saturating_sub(exponent as usize)
    } else {
        (3 - exponent) as usize
    };
    trim_float(format!("{value:.decimals$}"))
}

fn trim_float(mut value: String) -> String {
    if let Some(dot) = value.find('.') {
        while value.ends_with('0') {
            value.pop();
        }
        if value.len() == dot + 1 {
            value.pop();
        }
    }
    if value == "-0" {
        "0".to_string()
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use crate::core::werka::models::{ArchiveTotalByUom, WerkaArchiveSummary};

    use super::*;

    #[test]
    fn pdf_starts_with_header_and_contains_archive_lines() {
        let data = WerkaArchiveResponse {
            kind: "sent".to_string(),
            period: "monthly".to_string(),
            from: "2026-01-16".to_string(),
            to: "2026-01-20".to_string(),
            summary: WerkaArchiveSummary {
                record_count: 1,
                totals_by_uom: vec![ArchiveTotalByUom {
                    uom: "Kg".to_string(),
                    qty: 12.5,
                }],
            },
            items: vec![DispatchRecord {
                id: "DN-001".to_string(),
                record_type: "delivery_note".to_string(),
                supplier_name: "Customer".to_string(),
                item_code: "ITEM-001".to_string(),
                item_name: "Item".to_string(),
                uom: "Kg".to_string(),
                sent_qty: 12.5,
                accepted_qty: 10.0,
                status: "partial".to_string(),
                created_label: "2026-01-16".to_string(),
                ..DispatchRecord::default()
            }],
        };

        let pdf = String::from_utf8(build_archive_pdf(&data)).expect("pdf text");

        assert!(pdf.starts_with("%PDF-1.4\n"));
        assert!(pdf.contains("Werka Archive Report"));
        assert!(pdf.contains("Kind: sent"));
        assert!(pdf.contains("Records: 1"));
        assert!(pdf.ends_with("%%EOF\n"));
    }

    #[test]
    fn escape_pdf_text_matches_go_rules() {
        assert_eq!(escape_pdf_text("  a(b)\\c\tЯ  "), "a\\(b\\)\\\\c ?");
    }

    #[test]
    fn four_g_float_format_matches_go_examples() {
        assert_eq!(format_go_4g(12.5), "12.5");
        assert_eq!(format_go_4g(1.2345), "1.234");
        assert_eq!(format_go_4g(0.001234), "0.001234");
        assert_eq!(format_go_4g(10000.0), "1e+04");
        assert_eq!(format_go_4g(0.00001), "1e-05");
    }
}
