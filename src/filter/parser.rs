use super::error::FilterParseError;
use std::str::FromStr;

/// Types of filters that can be applied
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterType {
    /// Filter by component name (e.g., "core-universal", "socket")
    Component,
    /// Filter by log level (e.g., "INFO", "ERROR")
    Level,
    /// Filter by text content in message
    Text,
    /// Filter by direction (incoming/outgoing)
    Direction,
    /// Filter by any structured field key=value extracted from the log entry
    StructuredField,
}

impl FromStr for FilterType {
    type Err = FilterParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "component" | "comp" | "c" => Ok(FilterType::Component),
            "level" | "lvl" | "l" => Ok(FilterType::Level),
            "text" | "t" => Ok(FilterType::Text),
            "direction" | "dir" | "d" => Ok(FilterType::Direction),
            _ => Ok(FilterType::StructuredField),
        }
    }
}

impl FilterType {
    /// Get the canonical name of this filter type
    pub fn canonical_name(&self) -> &'static str {
        match self {
            FilterType::Component => "component",
            FilterType::Level => "level",
            FilterType::Text => "text",
            FilterType::Direction => "direction",
            FilterType::StructuredField => "field",
        }
    }
}

/// A single filter term (e.g., "component:core" or "!level:DEBUG")
#[derive(Debug, Clone)]
pub struct FilterTerm {
    /// The type of filter
    pub filter_type: FilterType,
    /// Field key for structured-field filters
    pub field_key: Option<String>,
    /// The value to match
    pub value: String,
    /// Whether this is an exclusion filter (prefixed with !)
    pub exclude: bool,
}

impl FilterTerm {
    /// Parse a single filter term from a string
    pub fn parse(s: &str) -> Result<Self, FilterParseError> {
        let (exclude, rest) = if let Some(stripped) = s.strip_prefix('!') {
            (true, stripped)
        } else {
            (false, s)
        };

        let parts: Vec<&str> = rest.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(FilterParseError::InvalidExpression(format!(
                "Expected 'type:value' format, got: {}",
                s
            )));
        }

        let filter_type: FilterType = parts[0].parse()?;
        let field_key = (filter_type == FilterType::StructuredField).then(|| parts[0].to_string());
        let value = parts[1].trim().to_string();

        if value.is_empty() {
            return Err(FilterParseError::EmptyValue(
                filter_type.canonical_name().to_string(),
            ));
        }

        // Validate direction values
        if filter_type == FilterType::Direction {
            let lower = value.to_lowercase();
            if !matches!(lower.as_str(), "incoming" | "outgoing" | "in" | "out") {
                return Err(FilterParseError::InvalidDirection(value));
            }
        }

        Ok(FilterTerm {
            filter_type,
            field_key,
            value,
            exclude,
        })
    }
}

/// A complete filter expression consisting of multiple terms
#[derive(Debug, Clone, Default)]
pub struct FilterExpression {
    /// All filter terms (combined with AND logic)
    pub terms: Vec<FilterTerm>,
}

impl FilterExpression {
    /// Create a new empty filter expression
    pub fn new() -> Self {
        Self { terms: Vec::new() }
    }

    /// Parse a filter expression from a string
    ///
    /// Terms are separated by whitespace and combined with AND logic.
    pub fn parse(s: &str) -> Result<Self, FilterParseError> {
        let mut terms = Vec::new();

        // Split by whitespace, but handle quoted strings
        for part in split_preserving_quotes(s) {
            if !part.contains(':') {
                return Err(FilterParseError::InvalidExpression(format!(
                    "Expected 'type:value' format, got: {}",
                    part
                )));
            }
            terms.push(FilterTerm::parse(part)?);
        }

        Ok(FilterExpression { terms })
    }

    /// Check if this expression is empty (no filters)
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// Get all include filters of a specific type
    pub fn include_filters(&self, filter_type: &FilterType) -> Vec<&str> {
        self.terms
            .iter()
            .filter(|t| &t.filter_type == filter_type && !t.exclude)
            .map(|t| t.value.as_str())
            .collect()
    }

    /// Get all exclude filters of a specific type
    pub fn exclude_filters(&self, filter_type: &FilterType) -> Vec<&str> {
        self.terms
            .iter()
            .filter(|t| &t.filter_type == filter_type && t.exclude)
            .map(|t| t.value.as_str())
            .collect()
    }

    pub fn include_structured_filters(&self) -> Vec<(&str, &str)> {
        self.terms
            .iter()
            .filter(|t| t.filter_type == FilterType::StructuredField && !t.exclude)
            .filter_map(|t| Some((t.field_key.as_deref()?, t.value.as_str())))
            .collect()
    }

    pub fn exclude_structured_filters(&self) -> Vec<(&str, &str)> {
        self.terms
            .iter()
            .filter(|t| t.filter_type == FilterType::StructuredField && t.exclude)
            .filter_map(|t| Some((t.field_key.as_deref()?, t.value.as_str())))
            .collect()
    }
}

/// Split a string by whitespace while preserving quoted segments
fn split_preserving_quotes(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut in_quotes = false;
    let mut start = 0;

    for (i, c) in s.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            ' ' | '\t' if !in_quotes => {
                if i > start {
                    let part = &s[start..i];
                    if !part.trim().is_empty() {
                        parts.push(part.trim());
                    }
                }
                start = i + 1;
            }
            _ => {}
        }
    }

    // Add the last part
    if start < s.len() {
        let part = &s[start..];
        if !part.trim().is_empty() {
            parts.push(part.trim());
        }
    }

    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_filter() {
        let term = FilterTerm::parse("component:core").unwrap();
        assert_eq!(term.filter_type, FilterType::Component);
        assert_eq!(term.field_key, None);
        assert_eq!(term.value, "core");
        assert!(!term.exclude);
    }

    #[test]
    fn test_parse_exclude_filter() {
        let term = FilterTerm::parse("!level:DEBUG").unwrap();
        assert_eq!(term.filter_type, FilterType::Level);
        assert_eq!(term.field_key, None);
        assert_eq!(term.value, "DEBUG");
        assert!(term.exclude);
    }

    #[test]
    fn test_parse_short_aliases() {
        let term = FilterTerm::parse("c:core").unwrap();
        assert_eq!(term.filter_type, FilterType::Component);

        let term = FilterTerm::parse("l:ERROR").unwrap();
        assert_eq!(term.filter_type, FilterType::Level);

        let term = FilterTerm::parse("t:timeout").unwrap();
        assert_eq!(term.filter_type, FilterType::Text);

        let term = FilterTerm::parse("d:incoming").unwrap();
        assert_eq!(term.filter_type, FilterType::Direction);
    }

    #[test]
    fn test_parse_structured_field_filter() {
        let term = FilterTerm::parse("trace_id:abc123").unwrap();
        assert_eq!(term.filter_type, FilterType::StructuredField);
        assert_eq!(term.field_key.as_deref(), Some("trace_id"));
        assert_eq!(term.value, "abc123");
    }

    #[test]
    fn test_parse_expression() {
        let expr = FilterExpression::parse("component:core level:ERROR !text:timeout trace_id:abc")
            .unwrap();
        assert_eq!(expr.terms.len(), 4);
        assert_eq!(expr.include_filters(&FilterType::Component), vec!["core"]);
        assert_eq!(expr.include_filters(&FilterType::Level), vec!["ERROR"]);
        assert_eq!(expr.exclude_filters(&FilterType::Text), vec!["timeout"]);
        assert_eq!(expr.include_structured_filters(), vec![("trace_id", "abc")]);
    }

    #[test]
    fn test_invalid_direction() {
        let result = FilterTerm::parse("direction:invalid");
        assert!(result.is_err());
    }
}
