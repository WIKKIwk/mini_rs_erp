fn qolip_spec_matches_order(spec: &QolipProductSpec, expected: &QolipProduct) -> bool {
    let expected_group = expected.item_group.trim();
    if expected_group.is_empty() || !spec.item_group.trim().eq_ignore_ascii_case(expected_group) {
        return false;
    }
    let expected_code = expected.code.trim();
    if !expected_code.is_empty() {
        return spec.item_code.trim().eq_ignore_ascii_case(expected_code);
    }
    let expected_name = expected.name.trim();
    !expected_name.is_empty() && spec.item_name.trim().eq_ignore_ascii_case(expected_name)
}

fn qolip_location_matches_spec(location: &QolipLocation, spec: &QolipProductSpec) -> bool {
    location
        .qolip_code
        .trim()
        .eq_ignore_ascii_case(spec.qolip_code.trim())
        && location
            .item_code
            .trim()
            .eq_ignore_ascii_case(spec.item_code.trim())
        && location
            .item_name
            .trim()
            .eq_ignore_ascii_case(spec.item_name.trim())
        && location.size == spec.size
}
