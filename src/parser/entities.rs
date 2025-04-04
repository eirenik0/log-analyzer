use serde_json::Value;
use std::fmt;

/// Different types of log entries based on their purpose
#[derive(Debug, Clone)]
pub enum LogEntryKind {
    /// An event emission or reception
    Event {
        /// Type of the event (e.g., "Logger.log", "Core.makeCore")
        event_type: String,
        /// Whether this event was emitted or received
        direction: EventDirection,
        /// Optional JSON payload associated with the event
        payload: Option<Value>,
    },
    /// A command execution
    Command {
        /// Name of the command (e.g., "close")
        command: String,
        /// Optional settings for the command
        settings: Option<Value>,
    },
    /// An API request or response
    Request {
        /// Name of the request (e.g., "openEyes", "startRenders")
        request: String,
        /// Optional request ID
        request_id: Option<String>,
        /// Optional endpoint information
        endpoint: Option<String>,
        /// Whether the request is being sent or received
        direction: RequestDirection,
        /// Optional JSON payload
        payload: Option<Value>,
    },
    /// Any other type of log entry
    Generic {
        /// Optional JSON payload found in the message
        payload: Option<Value>,
    },
}

/// Direction of an event (emitted or received)
#[derive(Debug, Clone, PartialEq)]
pub enum EventDirection {
    Emit,
    Receive,
}

/// Direction of a request (sending or receiving a response)
#[derive(Debug, Clone, PartialEq)]
pub enum RequestDirection {
    Send,
    Receive,
}

impl fmt::Display for EventDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EventDirection::Emit => write!(f, "emit"),
            EventDirection::Receive => write!(f, "receive"),
        }
    }
}

impl fmt::Display for RequestDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RequestDirection::Send => write!(f, "send"),
            RequestDirection::Receive => write!(f, "receive"),
        }
    }
}

/// Main log entry structure with integrated base fields
#[derive(Debug, Clone)]
pub struct LogEntry {
    /// Component that generated the log (e.g., "core-universal", "socket", "driver")
    pub component: String,
    /// Optional component ID (e.g., "manager-ufg-43w/eyes-ufg-oer/check-ufg-jdx")
    pub component_id: String,
    /// Timestamp of the log entry
    pub timestamp: String,
    /// Log level (e.g., "INFO", "WARN", "ERROR")
    pub level: String,
    /// The clean message with JSON content removed
    pub message: String,
    /// The original, unaltered log line
    pub raw_logline: String,
    /// Specific variant of the log entry
    pub kind: LogEntryKind,
}

impl LogEntry {
    /// Get the payload regardless of the kind of log entry
    pub fn payload(&self) -> Option<&Value> {
        match &self.kind {
            LogEntryKind::Event { payload, .. } => payload.as_ref(),
            LogEntryKind::Command { settings, .. } => settings.as_ref(),
            LogEntryKind::Request { payload, .. } => payload.as_ref(),
            LogEntryKind::Generic { payload } => payload.as_ref(),
        }
    }

    /// Check if this log entry is an event with the given type
    pub fn is_event(&self, event_type: &str) -> bool {
        match &self.kind {
            LogEntryKind::Event { event_type: et, .. } => et == event_type,
            _ => false,
        }
    }

    /// Check if this log entry is a command with the given name
    pub fn is_command(&self, cmd: &str) -> bool {
        match &self.kind {
            LogEntryKind::Command { command, .. } => command == cmd,
            _ => false,
        }
    }

    /// Check if this log entry is a request with the given name
    pub fn is_request(&self, req: &str) -> bool {
        match &self.kind {
            LogEntryKind::Request { request, .. } => request == req,
            _ => false,
        }
    }

    /// Get the type of this log entry as a string
    pub fn entry_type(&self) -> &'static str {
        match self.kind {
            LogEntryKind::Event { .. } => "event",
            LogEntryKind::Command { .. } => "command",
            LogEntryKind::Request { .. } => "request",
            LogEntryKind::Generic { .. } => "generic",
        }
    }

    /// Get the type of this log entry as a string
    pub fn log_key(&self) -> String {
        match self.kind.clone() {
            LogEntryKind::Event {
                event_type,
                direction,
                ..
            } => format!("{event_type}_{direction}"),
            LogEntryKind::Command { command, .. } => command,
            LogEntryKind::Request {
                request, direction, ..
            } => format!("{request}_{direction}"),
            LogEntryKind::Generic { .. } => "generic".to_string(),
        }
    }
}

// Helper functions for creating log entries
pub fn create_event_log(
    component: String,
    component_id: String,
    timestamp: String,
    level: String,
    message: String,
    raw_logline: String,
    event_type: String,
    direction: EventDirection,
    payload: Option<Value>,
) -> LogEntry {
    LogEntry {
        component,
        component_id,
        timestamp,
        level,
        message,
        raw_logline,
        kind: LogEntryKind::Event {
            event_type,
            direction,
            payload,
        },
    }
}

pub fn create_command_log(
    component: String,
    component_id: String,
    timestamp: String,
    level: String,
    message: String,
    raw_logline: String,
    command: String,
    settings: Option<Value>,
) -> LogEntry {
    LogEntry {
        component,
        component_id,
        timestamp,
        level,
        message,
        raw_logline,
        kind: LogEntryKind::Command { command, settings },
    }
}

pub fn create_request_log(
    component: String,
    component_id: String,
    timestamp: String,
    level: String,
    message: String,
    raw_logline: String,
    request: String,
    request_id: Option<String>,
    endpoint: Option<String>,
    direction: RequestDirection,
    payload: Option<Value>,
) -> LogEntry {
    LogEntry {
        component,
        component_id,
        timestamp,
        level,
        message,
        raw_logline,
        kind: LogEntryKind::Request {
            request,
            request_id,
            endpoint,
            direction,
            payload,
        },
    }
}

pub fn create_generic_log(
    component: String,
    component_id: String,
    timestamp: String,
    level: String,
    message: String,
    raw_logline: String,
    payload: Option<Value>,
) -> LogEntry {
    LogEntry {
        component,
        component_id,
        timestamp,
        level,
        message,
        raw_logline,
        kind: LogEntryKind::Generic { payload },
    }
}
