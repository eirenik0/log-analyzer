//! Filter expression parsing and matching
//!
//! This module provides a unified filter expression syntax for filtering logs.
//! Instead of using multiple individual flags, users can specify filters using
//! a simple expression language.
//!
//! # Syntax
//!
//! ```text
//! type:value           Include logs matching this filter
//! !type:value          Exclude logs matching this filter
//! multiple terms       Different types combine with AND; same type values combine with OR
//! ```
//!
//! # Filter Types
//!
//! - `component:` / `comp:` / `c:` - Filter by component name
//! - `level:` / `lvl:` / `l:` - Filter by log level
//! - `text:` / `t:` - Filter by text in message
//! - `direction:` / `dir:` / `d:` - Filter by direction (incoming/outgoing)
//!
//! # Examples
//!
//! ```text
//! component:core                          # Logs from core component
//! level:ERROR                             # Error logs only
//! !level:DEBUG                            # Exclude debug logs
//! component:core level:ERROR              # Core errors
//! comp:core !text:timeout                 # Core logs without timeout
//! dir:incoming                            # Incoming requests/events
//! ```

pub mod error;
pub mod matcher;
pub mod parser;

pub use error::FilterParseError;
pub use matcher::{print_filter_warnings, to_log_filter};
pub use parser::{FilterExpression, FilterTerm, FilterType};
