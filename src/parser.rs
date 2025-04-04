use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

pub struct LogEntry {
    pub component: String,
    pub component_rest: String,
    pub timestamp: String,
    pub level: String,
    pub event_type: Option<String>,
    pub payload: Option<Value>,
    pub message: String,
    pub raw_logline: String,
}

/// Extracts JSON content from a log message
fn extract_json(input: &str) -> Option<Value> {
    // Common JSON indicators to look for
    const JSON_INDICATORS: [&str; 3] = ["with body [", "with body {", "with body"];

    // First try to extract JSON from known patterns
    for indicator in &JSON_INDICATORS {
        if let Some(marker_pos) = input.find(indicator) {
            let start_pos = marker_pos + indicator.len();

            // Find the start of the actual JSON content
            let json_start = if *indicator == "with body" {
                input[start_pos..]
                    .find(|c| c == '[' || c == '{')
                    .map(|i| start_pos + i)
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

/// Parses a log file into a vector of LogEntry structs
pub fn parse_log_file(path: &PathBuf) -> Result<Vec<LogEntry>, Box<dyn std::error::Error>> {
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
                if let Some(entry) = parse_log_entry(&log_text) {
                    logs.push(entry);
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
        if let Some(entry) = parse_log_entry(&log_text) {
            logs.push(entry);
        }
    }

    Ok(logs)
}

/// Parses a single log entry string into a LogEntry struct
pub fn parse_log_entry(log_text: &str) -> Option<LogEntry> {
    // Split the log by the first " | " delimiter
    let mut parts = log_text.splitn(2, " | ");

    // Extract component information
    let component_part = parts.next()?;
    let (component, component_rest) = extract_component_info(component_part);

    // Extract the rest of the log entry
    let rest = parts.next()?;

    // Extract timestamp, level, and message
    let (timestamp, level, message) = extract_log_parts(rest)?;

    // Process message for event type and payload
    let (event_type, payload) = extract_event_info(message);

    Some(LogEntry {
        component: component.to_string(),
        component_rest: component_rest.to_string(),
        timestamp: timestamp.to_string(),
        level: level.to_string(),
        event_type,
        payload,
        message: message.to_string(),
        raw_logline: log_text.to_string(),
    })
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
            let component_rest = &component_part[space_pos + 2..component_part.len() - 1];
            return (component, component_rest);
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

/// Extracts event type and payload from the message
fn extract_event_info(message: &str) -> (Option<String>, Option<Value>) {
    let mut event_type = None;
    let mut payload = None;

    // Check if message contains event information
    if message.contains("Emit event of type") || message.contains("Received event of type") {
        let event_parts: Vec<&str> = message.split("with payload").collect();
        if event_parts.len() >= 2 {
            // Extract event type
            event_type = extract_event_type(event_parts[0]);

            // Extract payload from the second part
            let payload_str = event_parts[1].trim();
            if payload_str.starts_with('{') {
                payload = json5::from_str::<Value>(payload_str).ok();
            }
        }
    } else {
        // Try to extract any JSON from the message
        payload = extract_json(message);
    }

    (event_type, payload)
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
