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

fn extract_json(input: &str) -> Option<Value> {
    // Try to find and extract valid JSON starting from different positions
    let mut search_position = 0;

    while search_position < input.len() {
        // Find the first potential JSON start from current search position
        let mut json_start = None;
        let mut in_string = false;
        let mut escape_next = false;
        let remaining = &input[search_position..];

        // Look for "with body" markers in different formats
        if let Some(marker_pos) = remaining.find("with body [") {
            json_start = Some(search_position + marker_pos + "with body ".len());
        } else if let Some(marker_pos) = remaining.find("with body {") {
            json_start = Some(search_position + marker_pos + "with body ".len());
        } else if let Some(marker_pos) = remaining.find("with body") {
            // General case for "with body" followed by JSON
            let start_pos = search_position + marker_pos + "with body".len();

            // Scan for the first { or [ after "with body"
            for (i, c) in input[start_pos..].char_indices() {
                if c == '{' || c == '[' {
                    json_start = Some(start_pos + i);
                    break;
                }
            }
        } else {
            // Look for the first unquoted { or [ in the input
            for (i, c) in remaining.char_indices() {
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
                    '{' | '[' => {
                        json_start = Some(search_position + i);
                        break;
                    }
                    _ => {}
                }
            }
        }

        // If no potential JSON start found, return None
        let start_index = match json_start {
            Some(idx) => idx,
            None => return None,
        };

        // Determine if we're dealing with an object or array
        let first_char = match input[start_index..].chars().next() {
            Some(c) if c == '{' || c == '[' => c,
            _ => {
                // Move search position past this non-JSON start and continue
                search_position = start_index + 1;
                continue;
            }
        };

        // Initialize counters
        let mut brace_count = 0;
        let mut bracket_count = 0;
        in_string = false;
        escape_next = false;
        let mut end_index = None;

        // Parse through to find matching end
        for (i, c) in input[start_index..].char_indices() {
            // Handle string content
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
                        end_index = Some(start_index + i + 1);
                        break;
                    }
                },
                '[' => bracket_count += 1,
                ']' => {
                    bracket_count -= 1;
                    if bracket_count == 0 && first_char == '[' && brace_count == 0 {
                        end_index = Some(start_index + i + 1);
                        break;
                    }
                },
                _ => {}
            }
        }

        // If we found a potential JSON substring
        if let Some(ei) = end_index {
            let json_str = input[start_index..ei].replace("undefined", "null");

            // Validate the JSON
            let valid_json = json5::from_str::<Value>(&json_str);
            if valid_json.is_ok(){
                return valid_json.ok();
            }
        }

        // If we didn't find valid JSON, move search position past this attempt
        search_position = start_index + 1;
    }

    // If we've searched the entire string and found no valid JSON
    None
}

pub fn parse_log_file(path: &PathBuf) -> Result<Vec<LogEntry>, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut logs = Vec::new();

    let mut current_log: Option<String> = None;

    for line in reader.lines() {
        let line = line?;

        // Check if this is a new log entry
        if let Some((_component, _rest)) = line.split_once(" | ") {
            // Save the previous log entry if it exists
            if let Some(log_text) = current_log.take() {
                if let Some(entry) = parse_log_entry(&log_text) {
                    logs.push(entry);
                }
            }

            // Start a new log entry
            current_log = Some(line.to_string());
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

pub fn parse_log_entry(log_text: &str) -> Option<LogEntry> {
    let mut parts = log_text.splitn(2, " | ");
    let component = parts.next()?;
    let (component, component_rest) = if let Some((f, s)) = component
        .split_once(' ')
        .map(|(a, b)| (a, &b.trim()[1..b.trim().len() - 1]))
    {
        (f, s)
    } else {
        (component, "")
    };
    let rest = parts.next()?;

    let timestamp_end = rest.find('[')? - 1;
    let timestamp = &rest[0..timestamp_end];

    let level_start = rest.find('[')? + 1;
    let level_end = rest.find(']')?;
    let level = &rest[level_start..level_end].trim();

    let message_start = level_end + 2;
    let message = &rest[message_start..];

    let mut event_type = None;
    let mut payload = None;

    // Check if it's an event type message
    if message.contains("Emit event of type") || message.contains("Received event of type") {
        let event_parts: Vec<&str> = message.split("with payload").collect();
        if event_parts.len() >= 2 {
            // Extract event type
            if message.contains("\"name\":") {
                // Handle JSON event type format
                if let Some(start) = event_parts[0].find('{') {
                    if let Some(end) = event_parts[0].find('}') {
                        let type_json = &event_parts[0][start..=end];
                        if let Ok(v) = serde_json::from_str::<Value>(type_json) {
                            if let Some(name) = v.get("name") {
                                event_type = Some(name.as_str().unwrap_or("").to_string());
                            }
                        }
                    }
                }
            } else {
                // Handle string event type format
                if let Some(start) = event_parts[0].find('"') {
                    if let Some(end) = event_parts[0][start + 1..].find('"') {
                        event_type = Some(event_parts[0][start + 1..start + 1 + end].to_string());
                    }
                }
            }

            // Extract payload
            let payload_str = event_parts[1].trim();
            if payload_str.starts_with('{') {
                if let Ok(json_value) = json5::from_str::<Value>(payload_str) {
                    payload = Some(json_value);
                }
            }
        }
    } else {
        payload = extract_json(message)
    }

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
