use chrono::{Datelike, TimeZone, Timelike, Utc};
use std::collections::HashSet;

const FIELD_RANGES: [(i32, i32); 5] = [
    (0, 59), // minute
    (0, 23), // hour
    (1, 31), // day of month
    (1, 12), // month
    (0, 6),  // day of week (0 = Sunday)
];

fn parse_field(expr: &str, field_index: usize, full_expr: &str) -> Result<HashSet<i32>, String> {
    let (min, max) = FIELD_RANGES[field_index];
    let mut values = HashSet::new();

    for part in expr.split(',') {
        if part.is_empty() {
            return Err(format!("Invalid cron expression: {full_expr}"));
        }
        let mut split = part.splitn(2, '/');
        let body = split.next().unwrap_or("");
        let step_part = split.next();
        let mut step = 1;
        if let Some(step_part) = step_part {
            if step_part.is_empty() || !step_part.chars().all(|c| c.is_ascii_digit()) {
                return Err(format!("Invalid cron expression: {full_expr}"));
            }
            step = step_part
                .parse::<i32>()
                .map_err(|_| format!("Invalid cron expression: {full_expr}"))?;
            if step < 1 {
                return Err(format!("Invalid cron expression: {full_expr}"));
            }
        }

        let mut from = min;
        let mut to = max;
        if body != "*" {
            let range: Vec<&str> = body.split('-').collect();
            match range.len() {
                1 => {
                    if range[0].is_empty() || !range[0].chars().all(|c| c.is_ascii_digit()) {
                        return Err(format!("Invalid cron expression: {full_expr}"));
                    }
                    from = range[0]
                        .parse::<i32>()
                        .map_err(|_| format!("Invalid cron expression: {full_expr}"))?;
                    to = if step_part.is_some() { max } else { from };
                }
                2 => {
                    if range[0].is_empty()
                        || range[1].is_empty()
                        || !range[0].chars().all(|c| c.is_ascii_digit())
                        || !range[1].chars().all(|c| c.is_ascii_digit())
                    {
                        return Err(format!("Invalid cron expression: {full_expr}"));
                    }
                    from = range[0]
                        .parse::<i32>()
                        .map_err(|_| format!("Invalid cron expression: {full_expr}"))?;
                    to = range[1]
                        .parse::<i32>()
                        .map_err(|_| format!("Invalid cron expression: {full_expr}"))?;
                }
                _ => return Err(format!("Invalid cron expression: {full_expr}")),
            }
        }

        if from < min || to > max || from > to {
            return Err(format!("Invalid cron expression: {full_expr}"));
        }

        let mut value = from;
        while value <= to {
            values.insert(value);
            match value.checked_add(step) {
                Some(next) => value = next,
                None => break,
            }
        }
    }

    Ok(values)
}

/// Whether `epoch_millis` (evaluated in UTC) matches the given 5-field cron
/// expression. Throws on malformed expressions.
pub fn matches_five_field_cron(expr: &str, epoch_millis: i64) -> Result<bool, String> {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    if fields.len() != 5 {
        return Err(format!("Invalid cron expression: {expr}"));
    }

    let minute = parse_field(fields[0], 0, expr)?;
    let hour = parse_field(fields[1], 1, expr)?;
    let dom = parse_field(fields[2], 2, expr)?;
    let month = parse_field(fields[3], 3, expr)?;
    let dow = parse_field(fields[4], 4, expr)?;

    let ts = Utc
        .timestamp_millis_opt(epoch_millis)
        .single()
        .ok_or("Invalid timestamp")?;

    if !month.contains(&(ts.month() as i32)) {
        return Ok(false);
    }
    if !minute.contains(&(ts.minute() as i32)) {
        return Ok(false);
    }
    if !hour.contains(&(ts.hour() as i32)) {
        return Ok(false);
    }
    // Standard cron: when both dom and dow are restricted, either may match.
    let dom_restricted = fields[2] != "*";
    let dow_restricted = fields[4] != "*";
    let dom_match = dom.contains(&(ts.day() as i32));
    // chrono weekday() numbers Mon=0..Sun=6; cron wants Sun=0..Sat=6.
    let cron_dow = (ts.weekday().num_days_from_sunday() as i32) % 7;
    let dow_match = dow.contains(&cron_dow);
    if dom_restricted && dow_restricted {
        return Ok(dom_match || dow_match);
    }
    Ok(dom_match && dow_match)
}
