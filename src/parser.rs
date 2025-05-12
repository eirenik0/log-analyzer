use chrono::{DateTime, Local};
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

mod entities;

pub use entities::{
    EventDirection, LogEntry, LogEntryKind, RequestDirection, create_command_log, create_event_log,
    create_generic_log, create_request_log,
};

/// Parse error types
#[derive(Debug)]
pub enum ParseError {
    IoError(std::io::Error),
    InvalidLogFormat(String),
    JsonParseError(String),
}

impl From<std::io::Error> for ParseError {
    fn from(err: std::io::Error) -> Self {
        ParseError::IoError(err)
    }
}

/// Parses a log file into a vector of LogEntry structs
pub fn parse_log_file(path: impl AsRef<Path>) -> Result<Vec<LogEntry>, ParseError> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut logs = Vec::new();

    let mut current_log: Option<String> = None;

    for line in reader.lines() {
        let line = line?;

        // Check if this is a new log entry (contains the separator " | ")
        if line.contains(" | ") {
            // Save the previous log entry if it exists
            if let Some(log_text) = current_log.take() {
                match parse_log_entry(&log_text) {
                    Ok(entry) => logs.push(entry),
                    Err(ParseError::InvalidLogFormat(_)) => {
                        // Skip invalid logs but don't stop processing
                        // Could log this if we had a logger
                    }
                    Err(e) => return Err(e),
                }
            }

            // Start a new log entry
            current_log = Some(line);
        } else if let Some(ref mut log_text) = current_log {
            // Continue the current log entry
            log_text.push('\n');
            log_text.push_str(&line);
        }
    }

    // Add the last log entry
    if let Some(log_text) = current_log {
        if let Ok(entry) = parse_log_entry(&log_text) {
            logs.push(entry);
        }
    }

    Ok(logs)
}

/// Parses a single log entry string into a LogEntry struct
pub fn parse_log_entry(log_text: &str) -> Result<LogEntry, ParseError> {
    // Split the log by the first " | " delimiter
    let mut parts = log_text.splitn(2, " | ");

    // Extract component information
    let component_part = parts
        .next()
        .ok_or_else(|| ParseError::InvalidLogFormat("Missing component section".to_string()))?;

    let (component, component_id) = extract_component_info(component_part);

    // Extract the rest of the log entry
    let rest = parts
        .next()
        .ok_or_else(|| ParseError::InvalidLogFormat("Missing log message section".to_string()))?;

    // Extract timestamp, level, and message
    let (timestamp, level, message) = extract_log_parts(rest)
        .ok_or_else(|| ParseError::InvalidLogFormat("Invalid log format".to_string()))?;

    let timestamp = timestamp.parse::<DateTime<Local>>().unwrap();
    // Process message to determine the log entry kind
    determine_log_entry_kind(
        component.to_string(),
        component_id.to_string(),
        timestamp,
        level.to_string(),
        message.to_string(),
        log_text.to_string(),
        message,
    )
}

/// Extracts component name and additional component info
fn extract_component_info(component_part: &str) -> (&str, &str) {
    if let Some(space_pos) = component_part.find(' ') {
        let component = &component_part[..space_pos];
        // Extract content within parentheses
        if component_part.len() > space_pos + 2
            && component_part.as_bytes()[space_pos + 1] == b'('
            && component_part.ends_with(')')
        {
            let component_id = &component_part[space_pos + 2..component_part.len() - 1];
            return (component, component_id);
        }
    }
    (component_part, "")
}

/// Extracts timestamp, log level, and message from the rest of the log
fn extract_log_parts(rest: &str) -> Option<(&str, &str, &str)> {
    let timestamp_end = rest.find('[')?;
    let timestamp = rest[..timestamp_end].trim();

    let level_start = timestamp_end + 1;
    let level_end = rest[level_start..].find(']')? + level_start;
    let level = &rest[level_start..level_end].trim();

    let message_start = level_end + 2;
    let message = if message_start < rest.len() {
        &rest[message_start..]
    } else {
        ""
    };

    Some((timestamp, level, message))
}

/// Determines the type of log entry based on the message content
fn determine_log_entry_kind(
    component: String,
    component_id: String,
    timestamp: DateTime<Local>,
    level: String,
    mut message_text: String,
    raw_logline: String,
    message: &str,
) -> Result<LogEntry, ParseError> {
    // Check for event logs
    if message.contains("Emit event of type") {
        let event_parts: Vec<&str> = message.split("with payload").collect();
        if event_parts.len() >= 2 {
            let event_type = extract_event_type(event_parts[0]).ok_or_else(|| {
                ParseError::InvalidLogFormat("Could not extract event type".to_string())
            })?;

            let payload_str = event_parts[1].trim();
            let payload = extract_json(payload_str);

            // Update cleaned message
            message_text = format!("{} with payload [JSON removed]", event_parts[0]);

            return Ok(create_event_log(
                component,
                component_id,
                timestamp,
                level,
                message_text,
                raw_logline,
                event_type,
                EventDirection::Emit,
                payload,
            ));
        }
    } else if message.contains("Received event of type") {
        let event_parts: Vec<&str> = message.split("with payload").collect();
        if event_parts.len() >= 2 {
            let event_type = extract_event_type(event_parts[0]).ok_or_else(|| {
                ParseError::InvalidLogFormat("Could not extract event type".to_string())
            })?;

            let payload_str = event_parts[1].trim();
            let payload = extract_json(payload_str);

            // Update cleaned message
            message_text = format!("{} with payload [JSON removed]", event_parts[0]);

            return Ok(create_event_log(
                component,
                component_id,
                timestamp,
                level,
                message_text,
                raw_logline,
                event_type,
                EventDirection::Receive,
                payload,
            ));
        }
    }
    // Check for command logs
    else if message.contains(r#"Command ""#) && message.contains(r#"" is called"#) {
        // Extract command name
        let cmd_prefix = r#"Command ""#;
        let cmd_suffix = r#"" is called"#;

        if let Some(start_idx) = message.find(cmd_prefix) {
            let cmd_name_start = start_idx + cmd_prefix.len();
            if let Some(end_idx) = message[cmd_name_start..].find(cmd_suffix) {
                let command = message[cmd_name_start..cmd_name_start + end_idx].to_string();

                // Try to extract settings payload
                let mut settings = None;
                let mut cleaned_message = message.to_string();

                for indicator in &["with settings"] {
                    if let Some(start_idx) = message.find(indicator) {
                        let settings_str = &message[start_idx + indicator.len() - 1..];
                        settings = extract_json(settings_str);

                        // Update cleaned message
                        cleaned_message = message[..start_idx].to_string();
                        cleaned_message.push_str(indicator);
                        cleaned_message.push_str(" [JSON removed]");
                        break;
                    }
                }

                message_text = cleaned_message;
                return Ok(create_command_log(
                    component,
                    component_id,
                    timestamp,
                    level,
                    message_text,
                    raw_logline,
                    command,
                    settings,
                ));
            }
        }
    }
    // Check for request logs
    else if message.contains(r#"Request ""#) {
        let (request_name, request_id, endpoint, direction, payload) =
            extract_request_info(message);

        if let Some(req_name) = request_name {
            // Clean request-related JSON
            let mut cleaned_message = message.to_string();
            for indicator in &["with settings", "with body"] {
                if let Some(start_idx) = message.find(indicator) {
                    cleaned_message = message[..start_idx].to_string();
                    cleaned_message.push_str(indicator);
                    cleaned_message.push_str(" [JSON removed]");
                    break;
                }
            }

            message_text = cleaned_message;

            return Ok(create_request_log(
                component,
                component_id,
                timestamp,
                level,
                message_text,
                raw_logline,
                req_name,
                request_id,
                endpoint,
                direction,
                payload,
            ));
        }
    }

    // Generic log entry (anything else)
    // Try to extract any JSON content from the message
    let payload = extract_json(message);

    // If we found JSON, clean the message
    if payload.is_some() {
        let mut cleaned_message = String::new();
        for (i, c) in message.char_indices() {
            if c == '{' || c == '[' && extract_json_from_position(message, i).is_some() {
                cleaned_message = message[..i].to_string();
                cleaned_message.push_str("[JSON removed]");
                break;
            }
        }

        if !cleaned_message.is_empty() {
            message_text = cleaned_message;
        }
    }

    Ok(create_generic_log(
        component,
        component_id,
        timestamp,
        level,
        message_text,
        raw_logline,
        payload,
    ))
}

/// Extracts request name, ID, endpoint and payload from messages containing request information
fn extract_request_info(
    message: &str,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    RequestDirection,
    Option<Value>,
) {
    let mut request_name = None;
    let mut request_id = None;
    let mut endpoint = None;
    let mut direction = RequestDirection::Send;
    let mut payload = None;

    // Extract request name
    let req_prefix = r#"Request ""#;
    if let Some(start_idx) = message.find(req_prefix) {
        let req_name_start = start_idx + req_prefix.len();
        if let Some(end_idx) = message[req_name_start..].find('"') {
            request_name = Some(message[req_name_start..req_name_start + end_idx].to_string());
        }
    }

    // Extract request ID - look for square brackets after the request name
    if let Some(id_start) = message.find('[') {
        if let Some(id_end) = message[id_start + 1..].find(']') {
            request_id = Some(message[id_start + 1..id_start + 1 + id_end].to_string());
        }
    }

    // Extract endpoint
    if let Some(addr_start) = message.find("address \"[") {
        let addr_content_start = addr_start + 9; // Skip "address \"["
        if let Some(addr_end) = message[addr_content_start..].find(']') {
            endpoint = Some(message[addr_content_start..addr_content_start + addr_end].to_string());
        }
    }

    // Determine direction
    if message.contains("will be sent") {
        direction = RequestDirection::Send;
    } else if message.contains("finished successfully") {
        direction = RequestDirection::Receive;
    }

    // Extract payload
    for indicator in &["with body"] {
        if let Some(start_idx) = message.find(indicator) {
            let body_content = &message[start_idx + indicator.len()..];
            payload = extract_json(body_content);
            break;
        }
    }

    (request_name, request_id, endpoint, direction, payload)
}

/// Extracts the event type string from the event part of the message
fn extract_event_type(event_part: &str) -> Option<String> {
    if event_part.contains("\"name\":") {
        // Handle JSON event type format
        if let Some(start) = event_part.find('{') {
            if let Some(end) = event_part.find('}') {
                let type_json = &event_part[start..=end];
                if let Ok(v) = serde_json::from_str::<Value>(type_json) {
                    if let Some(name) = v.get("name") {
                        return Some(name.as_str().unwrap_or("").to_string());
                    }
                }
            }
        }
    } else {
        // Handle string event type format
        if let Some(start) = event_part.find('"') {
            if let Some(end) = event_part[start + 1..].find('"') {
                return Some(event_part[start + 1..start + 1 + end].to_string());
            }
        }
    }

    None
}

/// Extracts JSON content from a log message
fn extract_json(input: &str) -> Option<Value> {
    // Common JSON indicators to look for
    const JSON_INDICATORS: [&str; 5] = [
        "with settings {",
        "with body [",
        "with body {",
        "with body",
        "with body ",
    ];

    // First try to extract JSON from known patterns
    for indicator in &JSON_INDICATORS {
        if let Some(marker_pos) = input.find(indicator) {
            let start_pos = marker_pos + indicator.len();

            // Find the start of the actual JSON content
            let json_start = if *indicator == "with body" || *indicator == "with body " {
                // Find the first occurrence of an opening brace or bracket
                let mut idx = None;
                for (i, c) in input[start_pos..].char_indices() {
                    if c == '[' || c == '{' {
                        idx = Some(start_pos + i);
                        break;
                    }
                }
                idx
            } else {
                // We want to include `[ {`
                Some(start_pos - 1)
            };

            if let Some(start_idx) = json_start {
                if let Some(json_value) = extract_json_from_position(input, start_idx) {
                    return Some(json_value);
                }
            }
        }
    }

    // If not found with indicators, try to find any JSON in the string
    for (i, c) in input.char_indices() {
        if c == '{' || c == '[' {
            if let Some(json_value) = extract_json_from_position(input, i) {
                return Some(json_value);
            }
        }
    }

    None
}

/// Attempts to extract valid JSON starting from a specific position
fn extract_json_from_position(input: &str, start_pos: usize) -> Option<Value> {
    if start_pos >= input.len() {
        return None;
    }

    // Determine if we're parsing an object or array
    let first_char = input[start_pos..].chars().next()?;
    if first_char != '{' && first_char != '[' {
        return None;
    }

    // Track nesting of braces and brackets
    let mut brace_count = 0;
    let mut bracket_count = 0;
    let mut in_string = false;
    let mut escape_next = false;

    for (i, c) in input[start_pos..].char_indices() {
        // Handle string content and escaping
        if in_string {
            if escape_next {
                escape_next = false;
                continue;
            }
            if c == '\\' {
                escape_next = true;
                continue;
            }
            if c == '"' {
                in_string = false;
            }
            continue;
        }

        match c {
            '"' => in_string = true,
            '{' => brace_count += 1,
            '}' => {
                brace_count -= 1;
                if brace_count == 0 && first_char == '{' && bracket_count == 0 {
                    // Found matching end for object
                    let json_str = input[start_pos..=start_pos + i].replace("undefined", "null");
                    return json5::from_str::<Value>(&json_str).ok();
                }
            }
            '[' => bracket_count += 1,
            ']' => {
                bracket_count -= 1;
                if bracket_count == 0 && first_char == '[' && brace_count == 0 {
                    // Found matching end for array
                    let json_str = input[start_pos..=start_pos + i].replace("undefined", "null");
                    return json5::from_str::<Value>(&json_str).ok();
                }
            }
            _ => {}
        }
    }

    None
}
