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
    pub command: Option<String>,
    pub request: Option<String>,
    pub request_rest: Option<String>, // Added field for request ID/rest info
    pub payload: Option<Value>,
    pub message: String,
    pub raw_logline: String,
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
    let (event_type, command, request, request_id, payload) = extract_event_info(message);

    let message_without_payload = extract_message_without_payload(message, &payload);
    Some(LogEntry {
        component: component.to_string(),
        component_rest: component_rest.to_string(),
        timestamp: timestamp.to_string(),
        level: level.to_string(),
        command,
        event_type,
        payload,
        request,
        request_rest: request_id,
        message: message_without_payload.to_string(),
        raw_logline: log_text.to_string(),
    })
}

/// Extract the message text without the JSON payload
fn extract_message_without_payload(message: &str, payload: &Option<Value>) -> String {
    // If there's no payload, return the original message
    if payload.is_none() {
        return message.to_string();
    }

    let payload = payload.as_ref().unwrap();

    // Convert the payload to a string representation
    let payload_str = serde_json::to_string(payload).unwrap_or_default();

    // Try to find this payload in the message and remove it
    if let Some(idx) = message.find(&payload_str) {
        let without_payload = format!(
            "{}{}",
            &message[..idx].trim(),
            &message[idx + payload_str.len()..].trim()
        );
        return clean_message(without_payload);
    }

    // If we can't find the exact payload string, try a more flexible approach
    // by removing any JSON object/array from the message that could be the payload
    let json_objects = extract_all_json_objects(message);
    let mut result = message.to_string();

    for json_obj in json_objects {
        // Check if this JSON object is equivalent to our payload
        if let Ok(value) = serde_json::from_str::<Value>(&json_obj) {
            if value == *payload {
                result = result.replace(&json_obj, "");
                break;
            }
        }
    }

    clean_message(result)
}

/// Clean up a message string by removing common JSON markers and excess whitespace
fn clean_message(message: String) -> String {
    let mut result = message;

    // Cleanup any remaining JSON markers or whitespace
    let markers = [
        "with payload",
        "with body",
        "with settings",
        "with parameters",
        "will be sent to the address",
        "finished successfully",
    ];

    for marker in &markers {
        result = result.replace(marker, "");
    }

    // Also clean up any remaining square bracket content that might be URLs
    if let Some(start) = result.find("[POST]") {
        if let Some(end) = result[start..].find("\"") {
            result = format!("{}{}", &result[..start], &result[start + end..]);
        }
    }

    // Clean up multiple spaces and trim
    while result.contains("  ") {
        result = result.replace("  ", " ");
    }

    result.trim().to_string()
}

// Add this helper function if it's not already in parser.rs
fn extract_all_json_objects(text: &str) -> Vec<String> {
    let mut results = Vec::new();
    let mut start_indices = Vec::new();

    // Find all potential JSON object start positions
    for (i, c) in text.char_indices() {
        if c == '{' || c == '[' {
            start_indices.push(i);
        }
    }

    // For each start position, try to extract a valid JSON object
    for &start_idx in &start_indices {
        if let Some(end_idx) = find_json_end(text, start_idx) {
            let json_str = &text[start_idx..=end_idx];
            // Only add if it parses as valid JSON
            if serde_json::from_str::<Value>(json_str).is_ok() {
                results.push(json_str.to_string());
            }
        }
    }

    results
}

/// Finds the end index of a JSON object or array starting at start_idx
fn find_json_end(text: &str, start_idx: usize) -> Option<usize> {
    let first_char = text[start_idx..].chars().next()?;
    if first_char != '{' && first_char != '[' {
        return None;
    }

    let mut brace_count = 0;
    let mut bracket_count = 0;
    let mut in_string = false;
    let mut escape_next = false;

    for (i, c) in text[start_idx..].char_indices() {
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
                    return Some(start_idx + i);
                }
            }
            '[' => bracket_count += 1,
            ']' => {
                bracket_count -= 1;
                if bracket_count == 0 && first_char == '[' && brace_count == 0 {
                    return Some(start_idx + i);
                }
            }
            _ => {}
        }
    }

    None
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

/// Extracts event type, command, request and payload from the message
fn extract_event_info(
    message: &str,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<Value>,
) {
    let mut payload_str = message;
    let mut event_type = None;
    let mut command = None;
    let mut request = None;
    let mut request_rest = None;

    // Check if message contains event information
    if message.contains("Emit event of type") || message.contains("Received event of type") {
        let event_parts: Vec<&str> = message.split("with payload").collect();
        if event_parts.len() >= 2 {
            event_type = extract_event_type(event_parts[0]);
            payload_str = event_parts[1].trim();
        }
    } else if message.contains(r#"Command ""#) && message.contains(r#"" is called"#) {
        // Extract command name
        let cmd_prefix = r#"Command ""#;
        let cmd_suffix = r#"" is called"#;

        if let Some(start_idx) = message.find(cmd_prefix) {
            let cmd_name_start = start_idx + cmd_prefix.len();
            if let Some(end_idx) = message[cmd_name_start..].find(cmd_suffix) {
                command = Some(message[cmd_name_start..cmd_name_start + end_idx].to_string());
            }
        }
    } else if message.contains(r#"Request ""#) {
        // Extract request name and rest information
        let (req_name, req_id) = extract_request_info(message);
        request = req_name;
        request_rest = req_id;
    }

    // Try to extract JSON payload regardless of the message pattern
    let payload = extract_json(payload_str);
    (event_type, command, request, request_rest, payload)
}

/// Extracts request name and ID from messages containing request information
fn extract_request_info(message: &str) -> (Option<String>, Option<String>) {
    // Pattern: Request "name" [id]
    let req_prefix = r#"Request ""#;

    if let Some(start_idx) = message.find(req_prefix) {
        // Extract request name
        let req_name_start = start_idx + req_prefix.len();
        if let Some(end_idx) = message[req_name_start..].find('"') {
            let request_name = message[req_name_start..req_name_start + end_idx].to_string();

            // Extract request ID - look for square brackets after the request name
            let rest_pos = req_name_start + end_idx + 1;
            if rest_pos < message.len() {
                let rest_of_message = &message[rest_pos..];

                // Find the opening bracket
                if let Some(id_start) = rest_of_message.find('[') {
                    // Find the matching closing bracket
                    if let Some(id_end) = rest_of_message[id_start + 1..].find(']') {
                        let request_id =
                            rest_of_message[id_start + 1..id_start + 1 + id_end].to_string();
                        return (Some(request_name), Some(request_id));
                    }
                }
            }

            // Return just the request name if no ID is found
            return (Some(request_name), None);
        }
    }

    (None, None)
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
