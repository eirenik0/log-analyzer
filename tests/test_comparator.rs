#[cfg(test)]
mod tests {
    use chrono::{DateTime, Local};
    use log_analyzer::comparator::LogFilter;
    use log_analyzer::{ComparisonOptions, LogEntry, LogEntryKind, compare_json, compare_logs};
    use serde_json::json;
    use tempfile::NamedTempFile;

    // Import existing tests
    #[test]
    fn test_compare_json_with_different_order() {
        let json1 = json!({
            "height": 800,
            "name": "chrome",
            "width": 400
        });

        let json2 = json!({
            "width": 1000,
            "height": 800,
            "name": "chrome"
        });

        let diff = compare_json(&json1, &json2);

        // Should only find one difference: width
        assert_eq!(diff.len(), 1);
        assert_eq!(diff[0].0, "width");
        assert_eq!(diff[0].1, json!(400));
        assert_eq!(diff[0].2, json!(1000));
    }

    #[test]
    fn test_compare_json_with_array_of_objects() {
        let json1 = json!([
            { "id": 1, "name": "item1" },
            { "id": 2, "name": "item2" }
        ]);

        let json2 = json!([
            { "name": "item2", "id": 2 },
            { "name": "item1", "id": 1 }
        ]);

        let diff = compare_json(&json1, &json2);

        // Should find no differences despite different order
        assert_eq!(diff.len(), 0);
    }

    #[test]
    fn test_compare_json_with_array_of_objects_with_differences() {
        let json1 = json!([
            { "id": 1, "name": "item1", "value": 100 },
            { "id": 2, "name": "item2", "value": 200 }
        ]);

        let json2 = json!([
            { "name": "item2", "id": 2, "value": 250 },
            { "name": "item1", "id": 1, "value": 100 }
        ]);

        let diff = compare_json(&json1, &json2);

        // Should find exactly one difference (value: 200 vs 250)
        assert_eq!(diff.len(), 1);
        assert_eq!(diff[0].0, "[1].value");
        assert_eq!(diff[0].1, json!(200));
        assert_eq!(diff[0].2, json!(250));
    }

    #[test]
    fn test_compare_json_with_array_of_objects_with_differences_recursively() {
        let json1 = json!([
        {
          "environments": [
            { "height": 800, "name": "chrome", "width": 400 },
            { "height": 800, "name": "chrome", "width": 1000 }
          ]
        }]);

        let json2 = json!([
        {
          "environments": [
            { "width": 400, "height": 800, "name": "chrome" },
            { "width": 1000, "height": 800, "name": "chrome" }
          ]
        }]);
        let diff = compare_json(&json1, &json2);
        assert_eq!(diff.len(), 0);
    }

    #[test]
    fn test_compare_json_with_nested_objects() {
        let json1 = json!({
            "user": {
                "id": 1,
                "profile": {
                    "name": "Alice",
                    "age": 30
                }
            }
        });

        let json2 = json!({
            "user": {
                "id": 1,
                "profile": {
                    "name": "Alice",
                    "age": 31
                }
            }
        });

        let diff = compare_json(&json1, &json2);

        assert_eq!(diff.len(), 1);
        assert_eq!(diff[0].0, "user.profile.age");
        assert_eq!(diff[0].1, json!(30));
        assert_eq!(diff[0].2, json!(31));
    }

    #[test]
    fn test_compare_json_with_missing_fields() {
        let json1 = json!({
            "user": {
                "id": 1,
                "name": "Alice",
                "email": "alice@example.com"
            }
        });

        let json2 = json!({
            "user": {
                "id": 1,
                "name": "Alice"
            }
        });

        let diff = compare_json(&json1, &json2);

        assert_eq!(diff.len(), 1);
        assert_eq!(diff[0].0, "user.email");
        assert_eq!(diff[0].1, json!("alice@example.com"));
        assert_eq!(diff[0].2, json!(null));
    }

    #[test]
    fn test_compare_json_with_added_fields() {
        let json1 = json!({
            "user": {
                "id": 1,
                "name": "Alice"
            }
        });

        let json2 = json!({
            "user": {
                "id": 1,
                "name": "Alice",
                "email": "alice@example.com"
            }
        });

        let diff = compare_json(&json1, &json2);

        assert_eq!(diff.len(), 1);
        assert_eq!(diff[0].0, "user.email");
        assert_eq!(diff[0].1, json!(null));
        assert_eq!(diff[0].2, json!("alice@example.com"));
    }

    #[test]
    fn test_compare_json_with_different_types() {
        let json1 = json!({
            "id": 123,
            "active": true
        });

        let json2 = json!({
            "id": "123",
            "active": "yes"
        });

        let diff = compare_json(&json1, &json2);

        assert_eq!(diff.len(), 2);
        // Check the differences contain both fields with type mismatches
        assert!(
            diff.iter()
                .any(|(path, v1, v2)| path == "id" && v1 == &json!(123) && v2 == &json!("123"))
        );
        assert!(
            diff.iter().any(|(path, v1, v2)| path == "active"
                && v1 == &json!(true)
                && v2 == &json!("yes"))
        );
    }

    #[test]
    fn test_compare_json_with_different_array_lengths() {
        let json1 = json!({
            "items": [1, 2, 3, 4]
        });

        let json2 = json!({
            "items": [1, 2, 3]
        });

        let diff = compare_json(&json1, &json2);

        assert_eq!(diff.len(), 1);
        assert_eq!(diff[0].0, "items[3]");
        assert_eq!(diff[0].1, json!(4));
        assert_eq!(diff[0].2, json!(null));
    }

    #[test]
    fn test_compare_json_with_empty_structures() {
        let json1 = json!({
            "empty_object": {},
            "empty_array": []
        });

        let json2 = json!({
            "empty_object": {"key": "value"},
            "empty_array": [1, 2]
        });

        let diff = compare_json(&json1, &json2);

        assert_eq!(diff.len(), 3);
        // Check for key difference in the previously empty object
        assert!(diff.iter().any(|(path, v1, v2)| path == "empty_object.key"
            && v1 == &json!(null)
            && v2 == &json!("value")));
        // Check for differences in the previously empty array
        assert!(diff.iter().any(|(path, v1, v2)| path == "empty_array[0]"
            && v1 == &json!(null)
            && v2 == &json!(1)));
        assert!(diff.iter().any(|(path, v1, v2)| path == "empty_array[1]"
            && v1 == &json!(null)
            && v2 == &json!(2)));
    }

    // E2E Tests for compare_logs

    #[test]
    fn test_compare_logs_with_different_components() {
        // Create sample log entries
        let logs1 = vec![
            create_log_entry("component1", "info", "message1", json!({"key": "value1"})),
            create_log_entry("component2", "warn", "message2", json!({"count": 5})),
        ];

        let logs2 = vec![
            create_log_entry("component1", "info", "message1", json!({"key": "value2"})),
            create_log_entry(
                "component3",
                "error",
                "message3",
                json!({"error": "File not found"}),
            ),
        ];

        // Create a temporary file for output
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Run comparison
        let filter = LogFilter::new();
        let options = ComparisonOptions::new().output_to_file(path.to_str());
        let result = compare_logs(&logs1, &logs2, &filter, &options);

        assert!(result.is_ok());

        // Read the output file and verify some expected content
        let content = std::fs::read_to_string(path).unwrap();

        // Should mention unique components
        assert!(content.contains("Log type only in file 1: component2_warn_"));
        assert!(content.contains("Log type only in file 2: component3_error_"));

        // Should mention the difference in value1 vs value2
        assert!(content.contains("key: String(\"value1\") => String(\"value2\")"));
    }

    #[test]
    fn test_compare_logs_with_filters() {
        // Create sample log entries with different components and levels
        let logs1 = vec![
            create_log_entry("frontend", "info", "UI loaded", json!({"loaded": true})),
            create_log_entry(
                "backend",
                "error",
                "Database connection failed",
                json!({"reason": "timeout"}),
            ),
            create_log_entry(
                "middleware",
                "warn",
                "Rate limit reached",
                json!({"limit": 100}),
            ),
        ];

        let logs2 = vec![
            create_log_entry("frontend", "info", "UI loaded", json!({"loaded": true})),
            create_log_entry(
                "backend",
                "error",
                "Database connection failed",
                json!({"reason": "auth"}),
            ),
            create_log_entry(
                "middleware",
                "warn",
                "Rate limit reached",
                json!({"limit": 120}),
            ),
        ];

        // Create a temporary file for output
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let filter = LogFilter::new().with_component(Some("backend"));
        let options = ComparisonOptions::new().output_to_file(path.to_str());

        // Run comparison with a component filter
        let result = compare_logs(&logs1, &logs2, &filter, &options);

        assert!(result.is_ok());

        // Read the output file and verify only backend logs were compared
        let content = std::fs::read_to_string(path).unwrap();

        // Should contain backend differences
        assert!(content.contains("reason: String(\"timeout\") => String(\"auth\")"));

        // Should NOT contain middleware differences (filtered out)
        assert!(!content.contains("limit: Number(100) => Number(120)"));
    }

    #[test]
    fn test_compare_logs_with_diff_only() {
        // Create sample log entries
        let logs1 = vec![
            create_log_entry(
                "component1",
                "info",
                "same message",
                json!({"same": "value"}),
            ),
            create_log_entry(
                "component2",
                "warn",
                "different message",
                json!({"different": 10}),
            ),
        ];

        let logs2 = vec![
            create_log_entry(
                "component1",
                "info",
                "same message",
                json!({"same": "value"}),
            ),
            create_log_entry(
                "component2",
                "warn",
                "different message",
                json!({"different": 20}),
            ),
        ];

        // Create a temporary file for output
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Run comparison with diff_only=true
        let filter = LogFilter::new();
        let options = ComparisonOptions::new()
            .diff_only(true)
            .output_to_file(path.to_str());
        let result = compare_logs(&logs1, &logs2, &filter, &options);

        assert!(result.is_ok());

        // Read the output and verify it only contains differences
        let content = std::fs::read_to_string(path).unwrap();

        // Should contain the difference
        assert!(content.contains("different: Number(10) => Number(20)"));

        // Should NOT have log types only in one file (they should be filtered out due to diff_only=true)
        assert!(!content.contains("Log type only in file"));
    }

    #[test]
    fn test_compare_logs_with_show_full() {
        // Create sample log entries
        let logs1 = vec![create_log_entry(
            "component1",
            "info",
            "message",
            json!({
                "key1": "value1",
                "key2": "same",
                "nested": {
                    "inner": 10
                }
            }),
        )];

        let logs2 = vec![create_log_entry(
            "component1",
            "info",
            "message",
            json!({
                "key1": "value2",
                "key2": "same",
                "nested": {
                    "inner": 20
                }
            }),
        )];

        // Create a temporary file for output
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Run comparison with show_full=true
        let filter = LogFilter::new();
        let options = ComparisonOptions::new()
            .show_full_json(true)
            .output_to_file(path.to_str());
        let result = compare_logs(&logs1, &logs2, &filter, &options);

        assert!(result.is_ok());

        // Read the output and verify it contains full JSON objects
        let content = std::fs::read_to_string(path).unwrap();

        // Should contain full JSON objects rather than just differences
        assert!(content.contains(r#"with json key: `"key1"`"#));
        assert!(content.contains(r#"- file1: "value1""#));
        assert!(content.contains(r#"- file2: "value2""#)); // Should include unchanged values too
    }

    // Helper function to create test log entries
    fn create_log_entry(
        component: &str,
        level: &str,
        message: &str,
        payload: serde_json::Value,
    ) -> LogEntry {
        let timestamp = "2023-01-01T00:00:00Z".parse::<DateTime<Local>>().unwrap();
        LogEntry {
            timestamp: timestamp,
            component: component.to_string(),
            component_id: "some-id".to_string(),
            level: level.to_string(),
            message: message.to_string(),
            kind: LogEntryKind::Generic {
                payload: Some(payload),
            },
            raw_logline: format!("{timestamp} {component} {message}"),
        }
    }
}
