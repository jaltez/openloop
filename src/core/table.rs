#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputFormat {
    Table,
    Json,
}

pub fn stdout_is_tty() -> bool {
    unsafe { libc::isatty(1) == 1 }
}

pub fn resolve_output_format(flag: Option<&str>) -> OutputFormat {
    match flag {
        Some("table") => OutputFormat::Table,
        Some("json") => OutputFormat::Json,
        _ => {
            if stdout_is_tty() {
                OutputFormat::Table
            } else {
                OutputFormat::Json
            }
        }
    }
}

/// Row value: column text plus optional per-cell ordering. Mirrors the loose
/// Record<string, string | number | null | undefined> of the TS table helper.
#[derive(Debug, Clone)]
pub struct Row(pub Vec<String>);

pub fn print_table(headers: &[&str], rows: &[Row]) {
    if rows.is_empty() {
        println!("(empty)");
        return;
    }

    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    for row in rows {
        for (i, cell) in row.0.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(cell.chars().count());
            }
        }
    }

    let pad = |text: &str, width: usize| -> String {
        let count = text.chars().count();
        if count >= width {
            text.chars().take(width).collect()
        } else {
            format!("{text}{}", " ".repeat(width - count))
        }
    };

    let header: String = headers
        .iter()
        .enumerate()
        .map(|(i, h)| pad(&h.to_uppercase(), widths[i]))
        .collect::<Vec<_>>()
        .join("  ");
    let divider: String = widths
        .iter()
        .map(|w| "-".repeat(*w))
        .collect::<Vec<_>>()
        .join("  ");
    println!("{header}");
    println!("{divider}");
    for row in rows {
        let line: String = row
            .0
            .iter()
            .enumerate()
            .map(|(i, cell)| pad(cell, *widths.get(i).unwrap_or(&0)))
            .collect::<Vec<_>>()
            .join("  ");
        println!("{line}");
    }
}
