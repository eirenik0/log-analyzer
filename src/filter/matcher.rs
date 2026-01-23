use super::parser::{FilterExpression, FilterType};
use crate::cli::Direction;
use crate::comparator::LogFilter;

/// Convert a FilterExpression to a LogFilter
///
/// This function translates the parsed filter expression into the
/// LogFilter struct used by the comparison and analysis functions.
pub fn to_log_filter(expr: &FilterExpression) -> LogFilter {
    let mut filter = LogFilter::new();

    // Process component filters
    let include_components = expr.include_filters(&FilterType::Component);
    if let Some(first) = include_components.first() {
        filter = filter.with_component(Some(*first));
    }

    let exclude_components = expr.exclude_filters(&FilterType::Component);
    if let Some(first) = exclude_components.first() {
        filter = filter.exclude_component(Some(*first));
    }

    // Process level filters
    let include_levels = expr.include_filters(&FilterType::Level);
    if let Some(first) = include_levels.first() {
        filter = filter.with_level(Some(*first));
    }

    let exclude_levels = expr.exclude_filters(&FilterType::Level);
    if let Some(first) = exclude_levels.first() {
        filter = filter.exclude_level(Some(*first));
    }

    // Process text filters
    let include_text = expr.include_filters(&FilterType::Text);
    if let Some(first) = include_text.first() {
        filter = filter.contains_text(Some(*first));
    }

    let exclude_text = expr.exclude_filters(&FilterType::Text);
    if let Some(first) = exclude_text.first() {
        filter = filter.excludes_text(Some(*first));
    }

    // Process direction filters
    let include_directions = expr.include_filters(&FilterType::Direction);
    if let Some(first) = include_directions.first() {
        let direction = parse_direction(first);
        filter = filter.with_direction(&direction);
    }

    filter
}

/// Parse a direction string into a Direction enum
fn parse_direction(s: &str) -> Option<Direction> {
    match s.to_lowercase().as_str() {
        "incoming" | "in" => Some(Direction::Incoming),
        "outgoing" | "out" => Some(Direction::Outgoing),
        _ => None,
    }
}

/// Print warnings for any unknown filter values
///
/// This helps users identify typos or unsupported values in their filters.
pub fn print_filter_warnings(expr: &FilterExpression) {
    // Check level values
    let known_levels = [
        "TRACE", "DEBUG", "INFO", "WARN", "WARNING", "ERROR", "FATAL",
    ];
    for level in expr.include_filters(&FilterType::Level) {
        if !known_levels.iter().any(|k| k.eq_ignore_ascii_case(level)) {
            eprintln!(
                "Warning: Unknown log level '{}'. Common levels are: {:?}",
                level, known_levels
            );
        }
    }
    for level in expr.exclude_filters(&FilterType::Level) {
        if !known_levels.iter().any(|k| k.eq_ignore_ascii_case(level)) {
            eprintln!(
                "Warning: Unknown log level '{}'. Common levels are: {:?}",
                level, known_levels
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_log_filter_basic() {
        let expr = FilterExpression::parse("component:core level:ERROR").unwrap();
        let _filter = to_log_filter(&expr);
        // LogFilter doesn't expose its internal state, so we can't easily test it
        // The real test is that it compiles and runs
    }

    #[test]
    fn test_parse_direction() {
        assert_eq!(parse_direction("incoming"), Some(Direction::Incoming));
        assert_eq!(parse_direction("INCOMING"), Some(Direction::Incoming));
        assert_eq!(parse_direction("in"), Some(Direction::Incoming));
        assert_eq!(parse_direction("outgoing"), Some(Direction::Outgoing));
        assert_eq!(parse_direction("out"), Some(Direction::Outgoing));
        assert_eq!(parse_direction("invalid"), None);
    }
}
