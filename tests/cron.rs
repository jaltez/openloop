use openloop::core::cron::matches_five_field_cron;

fn ts(y: i32, m: u32, d: u32, h: u32, min: u32) -> i64 {
    use chrono::TimeZone;
    chrono::Utc
        .with_ymd_and_hms(y, m, d, h, min, 0)
        .single()
        .unwrap()
        .timestamp_millis()
}

#[test]
fn matches_every_minute() {
    assert!(matches_five_field_cron("* * * * *", ts(2026, 1, 15, 10, 30)).unwrap());
}

#[test]
fn matches_exact_minute() {
    let expr = "30 10 * * *";
    assert!(matches_five_field_cron(expr, ts(2026, 1, 15, 10, 30)).unwrap());
    assert!(!matches_five_field_cron(expr, ts(2026, 1, 15, 10, 31)).unwrap());
    assert!(!matches_five_field_cron(expr, ts(2026, 1, 15, 11, 30)).unwrap());
}

#[test]
fn matches_step() {
    let expr = "*/15 * * * *";
    assert!(matches_five_field_cron(expr, ts(2026, 1, 15, 10, 0)).unwrap());
    assert!(matches_five_field_cron(expr, ts(2026, 1, 15, 10, 45)).unwrap());
    assert!(!matches_five_field_cron(expr, ts(2026, 1, 15, 10, 7)).unwrap());
}

#[test]
fn matches_range() {
    let expr = "0 9-17 * * *";
    assert!(matches_five_field_cron(expr, ts(2026, 1, 15, 9, 0)).unwrap());
    assert!(matches_five_field_cron(expr, ts(2026, 1, 15, 17, 0)).unwrap());
    assert!(!matches_five_field_cron(expr, ts(2026, 1, 15, 8, 0)).unwrap());
    assert!(!matches_five_field_cron(expr, ts(2026, 1, 15, 18, 0)).unwrap());
}

#[test]
fn matches_comma_list() {
    let expr = "5,35 * * * *";
    assert!(matches_five_field_cron(expr, ts(2026, 1, 15, 10, 5)).unwrap());
    assert!(matches_five_field_cron(expr, ts(2026, 1, 15, 10, 35)).unwrap());
    assert!(!matches_five_field_cron(expr, ts(2026, 1, 15, 10, 6)).unwrap());
}

#[test]
fn dom_and_dow_both_restricted_use_or_semantics() {
    // Standard cron: when both dom and dow are restricted, either may match.
    // 2026-01-15 is a Thursday (cron dow 4): dom=15 matches, dow=0 does not.
    let expr = "0 0 15 * 0";
    assert!(matches_five_field_cron(expr, ts(2026, 1, 15, 0, 0)).unwrap());
    // 2026-01-18 is a Sunday: dom=15 does not match, dow=0 does.
    assert!(matches_five_field_cron(expr, ts(2026, 1, 18, 0, 0)).unwrap());
    // 2026-01-16 Friday: neither matches.
    assert!(!matches_five_field_cron(expr, ts(2026, 1, 16, 0, 0)).unwrap());
}

#[test]
fn dom_only_restricted_requires_match() {
    let expr = "0 0 15 * *";
    assert!(matches_five_field_cron(expr, ts(2026, 1, 15, 0, 0)).unwrap());
    assert!(!matches_five_field_cron(expr, ts(2026, 1, 16, 0, 0)).unwrap());
}

#[test]
fn rejects_malformed_expressions() {
    for expr in [
        "* * * *",
        "* * * * * *",
        "60 * * * *",
        "* 24 * * *",
        "*/0 * * * *",
        "5-2 * * * *",
        "a * * * *",
    ] {
        assert!(
            matches_five_field_cron(expr, ts(2026, 1, 15, 10, 0)).is_err(),
            "expected error for {expr}"
        );
    }
}

#[test]
fn month_and_dow_boundaries() {
    assert!(matches_five_field_cron("0 0 1 1 *", ts(2026, 1, 1, 0, 0)).unwrap());
    assert!(!matches_five_field_cron("0 0 1 2 *", ts(2026, 1, 1, 0, 0)).unwrap());
    assert!(matches_five_field_cron("0 0 * 2 *", ts(2026, 2, 10, 0, 0)).unwrap());
    // 2026-01-18 is a Sunday (cron dow 0); 2026-01-19 is a Monday (dow 1).
    assert!(matches_five_field_cron("0 0 * * 0", ts(2026, 1, 18, 0, 0)).unwrap());
    assert!(matches_five_field_cron("0 0 * * 1", ts(2026, 1, 19, 0, 0)).unwrap());
    assert!(!matches_five_field_cron("0 0 * * 1", ts(2026, 1, 18, 0, 0)).unwrap());
}
