use chrono::{DateTime, Local};
use serde_json::Value;
use std::fmt;

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
            EventDirection::Emit => write!(f, "Emit"),
            EventDirection::Receive => write!(f, "Receive"),
        }
    }
}

impl fmt::Display for RequestDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RequestDirection::Send => write!(f, "Send"),
            RequestDirection::Receive => write!(f, "Receive"),
        }
    }
}

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

/// Main log entry structure with integrated base fields
#[derive(Debug, Clone)]
pub struct LogEntry {
    /// Component that generated the log (e.g., "core-universal", "socket", "driver")
    pub component: String,
    /// Optional component ID (e.g., "manager-ufg-43w/eyes-ufg-oer/check-ufg-jdx")
    pub component_id: String,
    /// Timestamp of the log entry
    pub timestamp: DateTime<Local>,
    /// Log level (e.g., "INFO", "WARN", "ERROR")
    pub level: String,
    /// The clean message with JSON content removed
    pub message: String,
    /// The original, unaltered log line
    pub raw_logline: String,
    /// Specific variant of the log entry
    pub kind: LogEntryKind,
    /// Source line number in the original file (1-indexed)
    pub source_line_number: usize,
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
            LogEntryKind::Event { .. } => "Event",
            LogEntryKind::Command { .. } => "Command",
            LogEntryKind::Request { .. } => "Request",
            LogEntryKind::Generic { .. } => "Generic",
        }
    }

    /// Get the type of this log entry as a string
    pub fn log_key(&self) -> String {
        let entry_type = self.entry_type();
        let key = match self.kind.clone() {
            LogEntryKind::Event {
                event_type,
                direction,
                ..
            } => format!("{direction} `{event_type}`"),
            LogEntryKind::Command { command, .. } => format!("`{command}`"),
            LogEntryKind::Request {
                request, direction, ..
            } => format!("{direction} `{request}`"),
            LogEntryKind::Generic { .. } => self.message.clone(),
        };
        format!("{entry_type}|{key}:")
    }
}

// Parameter structs for creating log entries
pub struct LogEntryBase {
    pub component: String,
    pub component_id: String,
    pub timestamp: DateTime<Local>,
    pub level: String,
    pub message: String,
    pub raw_logline: String,
    pub source_line_number: usize,
}

pub struct EventLogParams {
    pub base: LogEntryBase,
    pub event_type: String,
    pub direction: EventDirection,
    pub payload: Option<Value>,
}

pub struct CommandLogParams {
    pub base: LogEntryBase,
    pub command: String,
    pub settings: Option<Value>,
}

pub struct RequestLogParams {
    pub base: LogEntryBase,
    pub request: String,
    pub request_id: Option<String>,
    pub endpoint: Option<String>,
    pub direction: RequestDirection,
    pub payload: Option<Value>,
}

// Helper functions for creating log entries
pub fn create_event_log(params: EventLogParams) -> LogEntry {
    LogEntry {
        component: params.base.component,
        component_id: params.base.component_id,
        timestamp: params.base.timestamp,
        level: params.base.level,
        message: params.base.message,
        raw_logline: params.base.raw_logline,
        kind: LogEntryKind::Event {
            event_type: params.event_type,
            direction: params.direction,
            payload: params.payload,
        },
        source_line_number: params.base.source_line_number,
    }
}

pub fn create_command_log(params: CommandLogParams) -> LogEntry {
    LogEntry {
        component: params.base.component,
        component_id: params.base.component_id,
        timestamp: params.base.timestamp,
        level: params.base.level,
        message: params.base.message,
        raw_logline: params.base.raw_logline,
        kind: LogEntryKind::Command {
            command: params.command,
            settings: params.settings,
        },
        source_line_number: params.base.source_line_number,
    }
}

pub fn create_request_log(params: RequestLogParams) -> LogEntry {
    LogEntry {
        component: params.base.component,
        component_id: params.base.component_id,
        timestamp: params.base.timestamp,
        level: params.base.level,
        message: params.base.message,
        raw_logline: params.base.raw_logline,
        kind: LogEntryKind::Request {
            request: params.request,
            request_id: params.request_id,
            endpoint: params.endpoint,
            direction: params.direction,
            payload: params.payload,
        },
        source_line_number: params.base.source_line_number,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn create_generic_log(
    component: String,
    component_id: String,
    timestamp: DateTime<Local>,
    level: String,
    message: String,
    raw_logline: String,
    payload: Option<Value>,
    source_line_number: usize,
) -> LogEntry {
    LogEntry {
        component,
        component_id,
        timestamp,
        level,
        message,
        raw_logline,
        kind: LogEntryKind::Generic { payload },
        source_line_number,
    }
}
