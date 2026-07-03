use greentic_extension_sdk_contract::DescribeJson;

fn describe() -> DescribeJson {
    let raw = include_str!("../describe.json");
    serde_json::from_str(raw).expect("describe.json must deserialize into DescribeJson")
}

#[test]
fn declares_three_trigger_nodetypes() {
    let d = describe();
    let ids: Vec<&str> = d
        .contributions
        .node_types
        .iter()
        .map(|nt| nt.type_id.as_str())
        .collect();
    assert!(ids.contains(&"timer-trigger"));
    assert!(ids.contains(&"sms-trigger"));
    assert!(ids.contains(&"email-trigger"));
    // category is String (not Option<String>) — compare directly
    for nt in &d.contributions.node_types {
        assert_eq!(nt.category, "trigger", "{} category", nt.type_id);
    }
}

#[test]
fn every_runtime_ref_resolves_to_a_component_with_oci_ref() {
    let d = describe();
    for nt in &d.contributions.node_types {
        // runtime_ref is Option<ComponentId> — use the ComponentId directly as BTreeMap key
        let rr = nt
            .runtime_ref
            .as_ref()
            .unwrap_or_else(|| panic!("trigger {} needs runtime_ref", nt.type_id));
        let comp = d
            .runtime
            .components
            .get(rr)
            .unwrap_or_else(|| panic!("runtime_ref {} must resolve to a component", rr));
        let oci = comp
            .oci_ref
            .as_deref()
            .unwrap_or_else(|| panic!("component {} needs oci_ref", rr));
        assert!(oci.contains("ghcr.io/greenticai/packs/events/"), "{oci}");
    }
}
