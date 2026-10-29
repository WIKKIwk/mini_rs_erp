impl ProductionMapService {
    /// Reuse a released run's tooling only while its complete physical
    /// composition still matches the product's current catalog requirements.
    pub async fn reusable_qolips_for_resume(
        &self,
        apparatus: &str,
        order_id: &str,
        required: &[crate::core::qolip::QolipProductSpec],
    ) -> Result<Option<Vec<crate::core::qolip::QolipProductSpec>>, ProductionMapError> {
        let Some(session) = self
            .store
            .active_order_run_session(apparatus, order_id)
            .await?
        else {
            return Ok(None);
        };
        if !session.qolip_reacquisition_required() {
            return Ok(None);
        }
        Ok(reusable_specs(&session, required))
    }
}

fn reusable_specs(
    session: &OrderRunSession,
    required: &[crate::core::qolip::QolipProductSpec],
) -> Option<Vec<crate::core::qolip::QolipProductSpec>> {
    let lineage = super::progress::QolipLineage::from_payload(&session.payload_json)?;
    let selected = required.iter().find(|spec| {
        lineage
            .qolip_codes
            .iter()
            .any(|code| code.eq_ignore_ascii_case(&spec.qolip_code))
            && (lineage.qolip_set_id.is_empty() || spec.set_id() == lineage.qolip_set_id)
            && (!spec.is_extension || required.iter().all(|entry| entry.is_extension))
    })?;
    let specs = crate::core::qolip::selected_product_specs(required, &selected.qolip_code);
    let codes = |values: Vec<&str>| {
        values
            .into_iter()
            .map(|code| code.trim().to_lowercase())
            .collect::<std::collections::BTreeSet<_>>()
    };
    if specs.is_empty()
        || codes(specs.iter().map(|spec| spec.qolip_code.as_str()).collect())
            != codes(lineage.qolip_codes.iter().map(String::as_str).collect())
    {
        return None;
    }
    if let Some(snapshot) = lineage.qolip_set_snapshot {
        let members = snapshot.get("members")?.as_array()?;
        if members.len() != specs.len() {
            return None;
        }
        for spec in &specs {
            let member = members.iter().find(|member| {
                member
                    .get("qolip_code")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|code| code.eq_ignore_ascii_case(&spec.qolip_code))
            })?;
            if member.get("size")?.as_i64()? != i64::from(spec.size)
                || member.get("color")?.as_str()? != spec.color
                || member
                    .get("qolip_set_id")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|id| id != spec.set_id())
                || member
                    .get("is_extension")
                    .and_then(serde_json::Value::as_bool)
                    .is_some_and(|extension| extension != spec.is_extension)
            {
                return None;
            }
        }
    }
    Some(specs.into_iter().cloned().collect())
}

#[cfg(test)]
mod resume_tests {
    use super::*;
    use crate::core::qolip::QolipProductSpec;
    use serde_json::json;

    fn fixture() -> (OrderRunSession, Vec<QolipProductSpec>) {
        let spec = QolipProductSpec {
            qolip_code: "OLD".into(),
            qolip_set_id: "set-a".into(),
            size: 420,
            color: "Black".into(),
            ..Default::default()
        };
        let session = serde_json::from_value(json!({
            "session_id":"run","apparatus":"apparatus:default:bosma_8","order_id":"zakaz-test",
            "status":"paused","worker_role":"aparatchi","worker_ref":"worker","worker_display_name":"Worker",
            "started_at_unix":1,"updated_at_unix":2,
            "payload_json":{"qolip_set_id":"set-a","qolip_codes":["OLD"],
                "qolip_set_snapshot":{"members":[{"qolip_code":"OLD","qolip_set_id":"set-a",
                    "size":420,"color":"Black","is_extension":false}]}}
        })).unwrap();
        (session, vec![spec])
    }

    #[test]
    fn unchanged_history_reuses_only_its_own_alternative() {
        let (session, mut specs) = fixture();
        specs.push(QolipProductSpec {
            qolip_code: "ALTERNATIVE".into(),
            qolip_set_id: "set-b".into(),
            ..specs[0].clone()
        });
        let reused = reusable_specs(&session, &specs).unwrap();
        assert_eq!(reused.len(), 1);
        assert_eq!(reused[0].qolip_code, "OLD");
    }

    #[test]
    fn changed_members_metadata_or_set_require_fresh_confirmation() {
        let (session, specs) = fixture();
        for field in ["code", "set", "size", "color", "extension", "added"] {
            let mut changed = specs.clone();
            match field {
                "code" => changed[0].qolip_code = "NEW".into(),
                "set" => changed[0].qolip_set_id = "set-b".into(),
                "size" => changed[0].size += 1,
                "color" => changed[0].color = "Red".into(),
                "extension" => changed[0].is_extension = true,
                _ => changed.push(QolipProductSpec {
                    qolip_code: "EXTRA".into(),
                    ..specs[0].clone()
                }),
            }
            assert!(reusable_specs(&session, &changed).is_none(), "{field}");
        }
    }
}
