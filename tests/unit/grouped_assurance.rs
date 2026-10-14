use super::*;
use serde_json::json;

#[test]
fn grouped_assurance_covers_every_member_and_preserves_scope_boundaries() {
    let view: InvestigationView = serde_json::from_value(json!({
        "groups": [{
            "id": "observations", "title": "Observations",
            "select": [{"path": "/kind", "equals": "observation"}],
            "by": [{"label": "Kind", "path": "/kind"}]
        }]
    }))
    .unwrap();
    view.validate().unwrap();
    let mut findings = Vec::new();
    for (id, scope, level, source, subject) in [
        ("a", "scope-0", "link_exists", "validated", "not_requested"),
        (
            "b",
            "scope-0",
            "relations_only",
            "unavailable",
            "unavailable",
        ),
        ("c", "scope-0", "link_exists", "validated", "not_requested"),
        ("d", "scope-1", "source_bound", "validated", "not_requested"),
    ] {
        findings.push(json!({
            "id": id, "scope_id": scope, "kind": "observation", "evidence": [],
            "checked": {
                "scope": {"id": scope},
                "assurance": {"level": level, "source_binding": source, "subject_verification": subject},
                "subjects": [{"event": {"name": id}}]
            }
        }));
    }
    for unavailable_first in [false, true] {
        if unavailable_first {
            findings.swap(0, 1);
        }
        let report = json!({"findings": findings});
        let overview = finding_overview(&report, Some(&view), false);
        let group = overview
            .rendered
            .split_once("- [scope-0] Kind: observation — 3 findings\n")
            .unwrap()
            .1
            .split("\n- [")
            .next()
            .unwrap();
        let (aggregate, representative) = group.split_once("Representative finding only:").unwrap();
        assert!(aggregate.contains("Group assurance (2 findings): link_exists; source binding validated; intended subject not_requested"));
        assert!(aggregate.contains("Group assurance (1 findings): relations_only; source binding unavailable; intended subject unavailable"));
        assert!(!aggregate.contains("Subject:"));
        assert!(
            representative.contains("Other members' subjects, populations and limits are omitted")
        );
        assert!(representative.contains("Subject:"));
        assert!(
            overview
                .rendered
                .contains("- [scope-1] Kind: observation — 1 findings")
        );
        assert!(overview.rendered.contains("Group assurance (1 findings): source_bound; source binding validated; intended subject not_requested"));
        assert!(!overview.rendered.contains("— 4 findings"));
    }
}
