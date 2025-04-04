#[cfg(test)]
mod tests {
    use log_analyzer::compare_json;
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
    fn test_compare_json_with_array_of_objects_recursively() {
        let json1 = json!([
            { "id": 1, "name": "item1" },
            { "id": 2, "name": { "id": 2, "name": "item2" } }
        ]);

        let json2 = json!([
            { "name": {"name": "item2", "id": 2, }, "id": 2 },
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
}
