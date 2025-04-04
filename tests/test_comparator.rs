#[cfg(test)]
mod tests {
    use log_analyzer::{
        compare_json, extract_all_json_objects, is_only_json_formatting_difference,
    };
    use serde_json::json;

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
    fn test_is_only_json_formatting_difference() {
        let text1 = r#"Some text with JSON { "name": "chrome", "width": 400, "height": 800 } and more JSON { "a": 1, "b": 2 }"#;
        let text2 = r#"Some text with JSON { "width": 400, "height": 800, "name": "chrome" } and more JSON { "b": 2, "a": 1 }"#;

        assert!(is_only_json_formatting_difference(text1, text2));

        // Different values should not be considered formatting differences
        let text3 = r#"Some text with JSON { "name": "chrome", "width": 500, "height": 800 }"#;

        assert!(!is_only_json_formatting_difference(text1, text3));

        // Different text should not be considered formatting differences
        let text4 = r#"Different text with JSON { "name": "chrome", "width": 400, "height": 800 }"#;

        assert!(!is_only_json_formatting_difference(text1, text4));
    }

    #[test]
    fn test_extract_all_json_objects() {
        let text = r#"Text with { "name": "chrome" } and { "width": 400, "height": 800 }"#;
        let objects = extract_all_json_objects(text);

        assert_eq!(objects.len(), 2);
        assert_eq!(objects[0], r#"{ "name": "chrome" }"#);
        assert_eq!(objects[1], r#"{ "width": 400, "height": 800 }"#);
    }
}
