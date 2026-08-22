use crate::core::apparatus_standard::test_support::{TestApparatusSpec, canonical_draft};
use crate::core::apparatus_standard::{
    ApparatusId, ExecutionOperation, LegacyCutoverDraftEntry, LegacyCutoverDraftManifest,
    ProcessTechnology, build_cutover_manifest,
};

use super::fixtures::TestDatabase;

#[tokio::test]
async fn exact_cutover_is_deterministic_transactional_and_reconciled() {
    let database = TestDatabase::create_through("cutover_positive", 69).await;
    let service = database.service();
    let report = service
        .cutover_preflight()
        .await
        .expect("cutover preflight");
    assert!(report.blocking_issues.is_empty());
    assert_eq!(report.legacy_apparatuses.len(), 10);
    assert!(
        report
            .diagnostics
            .iter()
            .all(|diagnostic| { diagnostic.unresolved_rows == 0 && diagnostic.orphan_rows == 0 })
    );

    let draft = draft_manifest(&report, false);
    let manifest = build_cutover_manifest(&report, draft.clone()).expect("build manifest");
    assert_eq!(
        manifest,
        build_cutover_manifest(&report, draft).expect("repeat deterministic manifest")
    );
    let preview = service
        .preview_legacy_cutover(&report, manifest.clone())
        .await
        .expect("preview manifest");
    let applied = service
        .apply_legacy_cutover(manifest)
        .await
        .expect("apply exact cutover");
    assert_eq!(preview, applied);
    assert_eq!(applied.entries.len(), 10);

    let counts = sqlx::query_as::<_, (i64, i64, i64, i64, i64, i64, i64, i64)>(
        "SELECT
             (SELECT count(*) FROM mini_apparatus),
             (SELECT count(*) FROM mini_canonical_apparatus_identities),
             (SELECT count(*) FROM mini_canonical_apparatus_revisions),
             (SELECT count(*) FROM mini_canonical_apparatus_heads),
             (SELECT count(*) FROM mini_apparatus_queue_policies),
             (SELECT count(*) FROM mini_apparatus_material_rules),
             (SELECT count(*) FROM mini_apparatus_capacity_profiles),
             (SELECT count(*) FROM mini_canonical_apparatus_change_outbox)",
    )
    .fetch_one(&database.pool)
    .await
    .expect("reconciled counts");
    assert_eq!(counts, (10, 10, 10, 10, 10, 10, 10, 10));
    let drift: i64 =
        sqlx::query_scalar("SELECT count(*) FROM mini_canonical_apparatus_projection_drift")
            .fetch_one(&database.pool)
            .await
            .expect("projection drift");
    assert_eq!(drift, 0);
    database.close().await;
}

#[tokio::test]
async fn changed_snapshot_and_mid_transaction_uniqueness_failure_leave_no_partial_state() {
    let database = TestDatabase::create_through("cutover_negative", 69).await;
    let service = database.service();
    let report = service
        .cutover_preflight()
        .await
        .expect("cutover preflight");
    let manifest =
        build_cutover_manifest(&report, draft_manifest(&report, false)).expect("build manifest");
    sqlx::query(
        "INSERT INTO mini_apparatus_groups (id, name, payload_json)
         VALUES ('cutover-race', 'Cutover race', '{\"apparatuses\":[]}'::jsonb)",
    )
    .execute(&database.pool)
    .await
    .expect("change source snapshot");
    assert!(service.apply_legacy_cutover(manifest).await.is_err());
    assert_eq!(canonical_row_count(&database).await, 0);

    sqlx::query("DELETE FROM mini_apparatus_groups WHERE id = 'cutover-race'")
        .execute(&database.pool)
        .await
        .expect("restore source snapshot");
    let report = service.cutover_preflight().await.expect("new preflight");
    let manifest = build_cutover_manifest(&report, draft_manifest(&report, true))
        .expect("build conflicting manifest");
    assert!(service.apply_legacy_cutover(manifest).await.is_err());
    assert_eq!(canonical_row_count(&database).await, 0);
    database.close().await;
}

async fn canonical_row_count(database: &TestDatabase) -> i64 {
    sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM mini_canonical_apparatus_identities)
              + (SELECT count(*) FROM mini_canonical_apparatus_revisions)
              + (SELECT count(*) FROM mini_canonical_apparatus_heads)
              + (SELECT count(*) FROM mini_canonical_apparatus_change_outbox)",
    )
    .fetch_one(&database.pool)
    .await
    .expect("canonical row count")
}

pub(super) fn draft_manifest(
    report: &crate::core::apparatus_standard::CutoverPreflightReport,
    duplicate_physical_asset: bool,
) -> LegacyCutoverDraftManifest {
    let entries = report
        .legacy_apparatuses
        .iter()
        .enumerate()
        .map(|(index, legacy)| {
            let display_name = legacy
                .observed_identities
                .iter()
                .find(|value| !value.starts_with("apparatus:"))
                .map(String::as_str)
                .unwrap_or(legacy.apparatus_id.as_str());
            let mut draft = explicit_draft(&legacy.apparatus_id, display_name);
            if duplicate_physical_asset && index == 1 {
                draft.physical_asset_id = explicit_draft(
                    &report.legacy_apparatuses[0].apparatus_id,
                    "duplicate physical asset",
                )
                .physical_asset_id;
            }
            LegacyCutoverDraftEntry {
                legacy_apparatus_id: legacy.apparatus_id.clone(),
                canonical_draft: draft,
                committed_at_unix_ms: 1_800_000_000_000,
                actor_id: "operator:test-cutover".to_string(),
                command_id: format!("cutover:{}", legacy.apparatus_id),
            }
        })
        .collect();
    LegacyCutoverDraftManifest {
        manifest_version: 1,
        preflight_fingerprint: report.fingerprint,
        entries,
    }
}

fn explicit_draft(
    apparatus_id: &str,
    display_name: &str,
) -> crate::core::apparatus_standard::CanonicalApparatusDraft {
    let mut spec = match apparatus_id {
        "apparatus:default:asset-004" => TestApparatusSpec::operation(
            apparatus_id,
            display_name,
            ExecutionOperation::Laminate,
            ProcessTechnology::ExtrusionLamination,
        ),
        "apparatus:default:asset-005" => TestApparatusSpec::print(
            apparatus_id,
            display_name,
            ProcessTechnology::Flexographic,
            None,
        ),
        "apparatus:default:asset-007" | "apparatus:default:asset-008" => {
            TestApparatusSpec::laminate(apparatus_id, display_name)
        }
        "apparatus:default:asset-010" => TestApparatusSpec::cut(apparatus_id, display_name),
        "apparatus:default:holodniy_kley" => TestApparatusSpec::operation(
            apparatus_id,
            display_name,
            ExecutionOperation::Glue,
            ProcessTechnology::ColdGlue,
        ),
        "apparatus:default:paket" => TestApparatusSpec::package(apparatus_id, display_name),
        value if value.contains("bosma_7") => TestApparatusSpec::print(
            apparatus_id,
            display_name,
            ProcessTechnology::Rotogravure,
            Some(7),
        ),
        value if value.contains("bosma_8") => TestApparatusSpec::print(
            apparatus_id,
            display_name,
            ProcessTechnology::Rotogravure,
            Some(8),
        ),
        value if value.contains("bosma_9") => TestApparatusSpec::print(
            apparatus_id,
            display_name,
            ProcessTechnology::Rotogravure,
            Some(9),
        ),
        _ => panic!("fixture requires an explicit apparatus profile: {apparatus_id}"),
    };
    if spec.operation == ExecutionOperation::Print {
        spec.tooling_required = true;
    }
    let draft = canonical_draft(&spec);
    assert_eq!(
        ApparatusId::new(apparatus_id).expect("stable apparatus id"),
        ApparatusId::new(spec.apparatus_id).expect("fixture apparatus id")
    );
    draft
}
