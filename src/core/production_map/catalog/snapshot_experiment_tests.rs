use super::{compile_saved_maps, compile_saved_maps_reusing_programs, snapshot_fixture};

#[test]
fn bbtree_snapshot_cache_hit_and_mixed_misses_match_full_compilation() {
    let maps = snapshot_fixture::maps(40);
    let previous = compile_saved_maps(maps.clone());
    assert_eq!(previous.len(), maps.len());
    assert_eq!(
        compile_saved_maps_reusing_programs(maps.clone(), Some(&previous)),
        compile_saved_maps(maps.clone())
    );

    let mut changed = maps;
    changed[0].nodes[1].qty_formula = "order_qty * 1.15".into();
    changed[1].title.push_str(" revised");
    changed[2].id = "zakaz-research-new".into();
    assert_eq!(
        compile_saved_maps_reusing_programs(changed.clone(), Some(&previous)),
        compile_saved_maps(changed)
    );
}

#[test]
fn bbtree_snapshot_no_previous_and_empty_previous_match_full_compilation() {
    let maps = snapshot_fixture::maps(6);
    let expected = compile_saved_maps(maps.clone());
    assert_eq!(
        compile_saved_maps_reusing_programs(maps.clone(), None),
        expected
    );
    assert_eq!(
        compile_saved_maps_reusing_programs(maps, Some(&[])),
        expected
    );
}

#[test]
fn bbtree_snapshot_duplicate_previous_ids_keep_the_last_entry() {
    let incoming = snapshot_fixture::map(1);
    let mut first = compile_saved_maps([incoming.clone()]).pop().unwrap();
    let mut last = first.clone();
    // Distinct sentinels make the prior-index choice observable without
    // reimplementing its algorithm. The historical std index is last-wins.
    first.program.operations[0].op_code = "first-cached-program".into();
    last.program.operations[0].op_code = "last-cached-program".into();
    assert_eq!(
        compile_saved_maps_reusing_programs([incoming.clone()], Some(&[first, last.clone()])),
        vec![last]
    );

    let expected = compile_saved_maps([incoming.clone(), incoming.clone()]);
    assert_eq!(
        compile_saved_maps_reusing_programs([incoming.clone(), incoming], Some(&expected)),
        expected
    );
}

#[test]
fn bbtree_snapshot_stale_duplicate_entry_cannot_reuse_the_earlier_program() {
    let incoming = snapshot_fixture::map(2);
    let mut first = compile_saved_maps([incoming.clone()]).pop().unwrap();
    first.program.operations[0].op_code = "must-not-reuse-earlier-duplicate".into();
    let mut stale = first.clone();
    stale.map.title.push_str(" obsolete");
    let expected = compile_saved_maps([incoming.clone()]);
    assert_eq!(
        compile_saved_maps_reusing_programs([incoming], Some(&[first, stale])),
        expected
    );
}

#[test]
fn bbtree_snapshot_invalid_graphs_are_skipped_and_order_is_preserved() {
    let previous = compile_saved_maps(snapshot_fixture::maps(4));
    let first = snapshot_fixture::map(0);
    let mut cyclic = snapshot_fixture::map(1);
    cyclic.edges.push(super::ProductionMapEdge {
        from: "end".into(),
        to: "start".into(),
        branch: String::new(),
    });
    let mut missing_target = snapshot_fixture::map(2);
    missing_target.edges[0].to = "does-not-exist".into();
    let last = snapshot_fixture::map(3);
    let inputs = vec![first.clone(), cyclic, missing_target, last.clone()];
    let actual = compile_saved_maps_reusing_programs(inputs.clone(), Some(&previous));
    assert_eq!(actual, compile_saved_maps(inputs));
    assert_eq!(actual.len(), 2);
    assert_eq!(actual[0].map.id, first.id);
    assert_eq!(actual[1].map.id, last.id);
}

#[test]
fn bbtree_snapshot_legacy_code_is_normalized_before_cache_matching() {
    let incoming = snapshot_fixture::map(3);
    assert!(incoming.code.is_empty());
    let mut cached = compile_saved_maps([incoming.clone()]).pop().unwrap();
    assert_eq!(cached.map.code, incoming.order_number);
    cached.program.operations[0].op_code = "reuse-after-normalization".into();
    let actual = compile_saved_maps_reusing_programs([incoming], Some(&[cached.clone()]));
    assert_eq!(actual, vec![cached]);
}
