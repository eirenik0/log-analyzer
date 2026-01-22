use thiserror::Error;

/// Errors that can occur when parsing filter expressions
#[derive(Debug, Error)]
pub enum FilterParseError {
    #[error("Unknown filter type: '{0}'. Valid types are: component (c), level (l), text (t), direction (d)")]
    UnknownFilterType(String),

    #[error("Empty filter value for type '{0}'")]
    EmptyValue(String),

    #[error("Invalid direction value: '{0}'. Valid values are: incoming, outgoing")]
    InvalidDirection(String),

    #[error("Invalid filter expression: {0}")]
    InvalidExpression(String),
}
