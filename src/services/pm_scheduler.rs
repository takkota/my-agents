use crate::storage::FsStore;
use chrono::{DateTime, Datelike, Timelike, Utc};
use std::collections::HashMap;

pub enum PmSchedulerEvent {
    TriggerPm { project_id: String },
}

pub struct PmScheduler {
    store: FsStore,
    /// Tracks last trigger per project: (year, ordinal_day, minute_of_day)
    last_triggered: HashMap<String, (i32, u32, u32)>,
}

impl PmScheduler {
    pub fn new(store: FsStore) -> Self {
        Self {
            store,
            last_triggered: HashMap::new(),
        }
    }

    pub fn check_all(&mut self, now: DateTime<Utc>) -> Vec<PmSchedulerEvent> {
        let mut events = Vec::new();

        let projects = match self.store.list_projects() {
            Ok(p) => p,
            Err(_) => return events,
        };

        let current_minute = now.hour() * 60 + now.minute();
        let current_key = (now.year(), now.ordinal(), current_minute);

        for project in &projects {
            if !project.pm_enabled {
                continue;
            }
            let cron_expr = match &project.pm_cron_expression {
                Some(expr) => expr.clone(),
                None => continue,
            };
            if project.pm_agent_cli.is_none() {
                continue;
            }

            // Check if already triggered this minute
            if let Some(last) = self.last_triggered.get(&project.id) {
                if *last == current_key {
                    continue;
                }
            }

            if cron_matches(&cron_expr, &now) {
                self.last_triggered.insert(project.id.clone(), current_key);
                events.push(PmSchedulerEvent::TriggerPm {
                    project_id: project.id.clone(),
                });
            }
        }

        events
    }
}

/// Validate a cron expression. Returns Ok(()) if valid, Err with description if invalid.
pub fn validate_cron(expr: &str) -> Result<(), String> {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    if fields.len() != 5 {
        return Err(format!(
            "Expected 5 fields (minute hour dom month dow), got {}",
            fields.len()
        ));
    }
    let field_names = ["minute", "hour", "day-of-month", "month", "day-of-week"];
    let ranges = [(0, 59), (0, 23), (1, 31), (1, 12), (0, 6)];
    for (i, field) in fields.iter().enumerate() {
        if let Err(e) = validate_cron_field(field, ranges[i].0, ranges[i].1) {
            return Err(format!("Invalid {} field '{}': {}", field_names[i], field, e));
        }
    }
    Ok(())
}

fn validate_cron_field(field: &str, min: u32, max: u32) -> Result<(), String> {
    if field == "*" {
        return Ok(());
    }
    if field.contains(',') {
        for part in field.split(',') {
            validate_cron_field(part.trim(), min, max)?;
        }
        return Ok(());
    }
    if let Some(step_str) = field.strip_prefix("*/") {
        let step: u32 = step_str
            .parse()
            .map_err(|_| format!("'{}' is not a valid number", step_str))?;
        if step == 0 {
            return Err("step cannot be 0".to_string());
        }
        if step > max - min + 1 {
            return Err(format!(
                "step {} exceeds field range {}-{}",
                step, min, max
            ));
        }
        return Ok(());
    }
    if field.contains('-') {
        let parts: Vec<&str> = field.splitn(2, '-').collect();
        if parts.len() != 2 {
            return Err("invalid range".to_string());
        }
        let start: u32 = parts[0]
            .parse()
            .map_err(|_| format!("'{}' is not a valid number", parts[0]))?;
        let end: u32 = parts[1]
            .parse()
            .map_err(|_| format!("'{}' is not a valid number", parts[1]))?;
        if start > end {
            return Err(format!("range start {} > end {}", start, end));
        }
        if start < min || end > max {
            return Err(format!("range {}-{} outside valid range {}-{}", start, end, min, max));
        }
        return Ok(());
    }
    let n: u32 = field
        .parse()
        .map_err(|_| format!("'{}' is not a valid number", field))?;
    if n < min || n > max {
        return Err(format!("value {} outside valid range {}-{}", n, min, max));
    }
    Ok(())
}

/// Check if a 5-field cron expression matches the given datetime.
/// Fields: minute hour day_of_month month day_of_week
/// Supports: *, n, n-m, */n, n,m
pub fn cron_matches(expr: &str, now: &DateTime<Utc>) -> bool {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    if fields.len() != 5 {
        return false;
    }

    let minute = now.minute();
    let hour = now.hour();
    let dom = now.day();
    let month = now.month();
    let dow = now.weekday().num_days_from_sunday(); // 0=Sun, 6=Sat

    parse_cron_field(fields[0], minute, 0, 59)
        && parse_cron_field(fields[1], hour, 0, 23)
        && parse_cron_field(fields[2], dom, 1, 31)
        && parse_cron_field(fields[3], month, 1, 12)
        && parse_cron_field(fields[4], dow, 0, 6)
}

/// Parse a single cron field and check if the value matches.
/// Supports: * (any), n (exact), n-m (range), */n (step), n,m (list)
#[allow(clippy::only_used_in_recursion)]
fn parse_cron_field(field: &str, value: u32, min: u32, max: u32) -> bool {
    if field == "*" {
        return true;
    }

    // Handle comma-separated list
    if field.contains(',') {
        return field.split(',').any(|part| parse_cron_field(part.trim(), value, min, max));
    }

    // Handle */n (step)
    if let Some(step_str) = field.strip_prefix("*/") {
        if let Ok(step) = step_str.parse::<u32>() {
            if step == 0 {
                return false;
            }
            return (value - min).is_multiple_of(step);
        }
        return false;
    }

    // Handle n-m (range)
    if field.contains('-') {
        let parts: Vec<&str> = field.splitn(2, '-').collect();
        if parts.len() == 2 {
            if let (Ok(start), Ok(end)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
                return value >= start && value <= end;
            }
        }
        return false;
    }

    // Handle exact value
    if let Ok(n) = field.parse::<u32>() {
        return value == n;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn make_dt(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, minute, 0)
            .unwrap()
    }

    #[test]
    fn test_every_minute() {
        let dt = make_dt(2026, 3, 15, 10, 30);
        assert!(cron_matches("* * * * *", &dt));
    }

    #[test]
    fn test_specific_minute() {
        let dt = make_dt(2026, 3, 15, 10, 30);
        assert!(cron_matches("30 * * * *", &dt));
        assert!(!cron_matches("31 * * * *", &dt));
    }

    #[test]
    fn test_specific_hour_minute() {
        let dt = make_dt(2026, 3, 15, 10, 30);
        assert!(cron_matches("30 10 * * *", &dt));
        assert!(!cron_matches("30 11 * * *", &dt));
    }

    #[test]
    fn test_step() {
        let dt = make_dt(2026, 3, 15, 10, 30);
        assert!(cron_matches("*/5 * * * *", &dt)); // 30 % 5 == 0
        assert!(cron_matches("*/10 * * * *", &dt)); // 30 % 10 == 0
        assert!(!cron_matches("*/7 * * * *", &dt)); // 30 % 7 != 0
    }

    #[test]
    fn test_range() {
        let dt = make_dt(2026, 3, 15, 10, 30);
        assert!(cron_matches("25-35 * * * *", &dt));
        assert!(!cron_matches("0-29 * * * *", &dt));
    }

    #[test]
    fn test_list() {
        let dt = make_dt(2026, 3, 15, 10, 30);
        assert!(cron_matches("0,15,30,45 * * * *", &dt));
        assert!(!cron_matches("0,15,45 * * * *", &dt));
    }

    #[test]
    fn test_day_of_week() {
        // 2026-03-15 is a Sunday (dow=0)
        let dt = make_dt(2026, 3, 15, 10, 0);
        assert!(cron_matches("0 10 * * 0", &dt));
        assert!(!cron_matches("0 10 * * 1", &dt));
    }

    #[test]
    fn test_day_of_month() {
        let dt = make_dt(2026, 3, 15, 10, 0);
        assert!(cron_matches("0 10 15 * *", &dt));
        assert!(!cron_matches("0 10 16 * *", &dt));
    }

    #[test]
    fn test_month() {
        let dt = make_dt(2026, 3, 15, 10, 0);
        assert!(cron_matches("0 10 15 3 *", &dt));
        assert!(!cron_matches("0 10 15 4 *", &dt));
    }

    #[test]
    fn test_invalid_expr() {
        let dt = make_dt(2026, 3, 15, 10, 0);
        assert!(!cron_matches("invalid", &dt));
        assert!(!cron_matches("* * *", &dt));
    }

    #[test]
    fn test_combined() {
        // Every 15 minutes on weekdays at 9-17
        let dt = make_dt(2026, 3, 16, 9, 15); // Monday
        assert!(cron_matches("*/15 9-17 * * 1-5", &dt));
        let dt_sun = make_dt(2026, 3, 15, 9, 15); // Sunday
        assert!(!cron_matches("*/15 9-17 * * 1-5", &dt_sun));
    }

    #[test]
    fn test_validate_cron_valid() {
        assert!(validate_cron("* * * * *").is_ok());
        assert!(validate_cron("0 9 * * 1-5").is_ok());
        assert!(validate_cron("*/15 9-17 * * 1-5").is_ok());
        assert!(validate_cron("0,15,30,45 * * * *").is_ok());
    }

    #[test]
    fn test_validate_cron_invalid() {
        assert!(validate_cron("invalid").is_err());
        assert!(validate_cron("* * *").is_err()); // too few fields
        assert!(validate_cron("* * * * * *").is_err()); // too many fields
        assert!(validate_cron("60 * * * *").is_err()); // minute out of range
        assert!(validate_cron("* 25 * * *").is_err()); // hour out of range
        assert!(validate_cron("*/0 * * * *").is_err()); // step 0
        assert!(validate_cron("*/100 * * * *").is_err()); // step exceeds range
        assert!(validate_cron("30-10 * * * *").is_err()); // reversed range
    }
}
