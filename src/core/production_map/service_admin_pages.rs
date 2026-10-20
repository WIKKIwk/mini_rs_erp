use super::*;
use crate::core::apparatus_standard::{ExecutionOperation, RuntimeApparatusConfiguration};
use std::sync::Arc;

impl ProductionMapService {
    /// Preserve the catalogue's ordering without building every order's live
    /// controls, WIP or compiled program just to select a page.
    pub async fn admin_order_catalog(
        &self,
        unassigned_for: Option<&str>,
    ) -> Result<(Vec<ProductionMapDefinition>, BTreeSet<String>), ProductionMapError> {
        let mut maps = if let Some(apparatus) = unassigned_for {
            // Unassigned print alternatives are deliberately absent from the
            // worker queue. Read their map candidates before applying filters.
            self.store
                .maps_for_snapshot_scope(&[apparatus.to_string()], &[])
                .await?
        } else {
            self.store.maps().await?
        };
        maps.retain(|map| map.id.starts_with("zakaz-"));
        let ids = maps.iter().map(|map| map.id.clone()).collect::<Vec<_>>();
        let lifecycles = self.store.production_order_lifecycles(&ids).await?;
        maps.retain(|map| {
            lifecycles.get(&map.id).is_none_or(|record| {
                unassigned_for.is_some() || !record.status.is_terminal_for_material_assignment()
            }) && compile_map(map).is_ok()
        });
        Ok((maps, ids.into_iter().collect()))
    }

    pub async fn admin_move_page_configurations(
        &self,
    ) -> Result<Vec<Arc<RuntimeApparatusConfiguration>>, ProductionMapError> {
        Ok(self.snapshot_canonical_apparatuses().await)
    }

    /// Read eligibility mirrors the two transfer panes. Actual writes still
    /// run the existing transactional move/transfer validation.
    pub fn admin_move_page_matches(
        &self,
        map: &ProductionMapDefinition,
        source: &str,
        target: &str,
        configurations: &[Arc<RuntimeApparatusConfiguration>],
    ) -> bool {
        let nodes = map
            .nodes
            .iter()
            .filter(|n| n.kind == ProductionMapNodeKind::Apparatus)
            .collect::<Vec<_>>();
        let unassigned = |apparatus: &str| {
            nodes.iter().any(|node| {
                node.apparatus_id == apparatus
                    && !node.alternative_group_id.is_empty()
                    && nodes
                        .iter()
                        .filter(|other| other.alternative_group_id == node.alternative_group_id)
                        .all(|other| other.alternative_assigned_apparatus_id.is_empty())
            })
        };
        if source == "unassigned" {
            return unassigned(target);
        }
        let assigned = nodes
            .iter()
            .filter(|node| {
                !node.alternative_group_id.is_empty()
                    && node.alternative_assigned_apparatus_id == source
            })
            .map(|node| node.alternative_group_id.as_str())
            .collect::<BTreeSet<_>>();
        if target == "unassigned" {
            return !assigned.is_empty() || unassigned(source);
        }
        let source_nodes = nodes
            .iter()
            .filter(|node| {
                node.apparatus_id == source || node.alternative_assigned_apparatus_id == source
            })
            .collect::<Vec<_>>();
        if source_nodes.is_empty() {
            return false;
        }
        let Some(from) = configurations
            .iter()
            .find(|c| c.runtime.apparatus_id.as_str() == source)
        else {
            return false;
        };
        let Some(to) = configurations
            .iter()
            .find(|c| c.runtime.apparatus_id.as_str() == target)
        else {
            return false;
        };
        let from_profile = &from.runtime.execution_profile;
        let to_profile = &to.runtime.execution_profile;
        if from_profile.operation != to_profile.operation
            || (from_profile.operation != ExecutionOperation::Print
                && from_profile.technology != to_profile.technology)
        {
            return false;
        }
        if source_nodes
            .iter()
            .any(|node| !node.alternative_group_id.is_empty())
        {
            // Preserve the existing UI's assigned/unassigned alternative rules.
            return assigned.is_empty()
                || nodes.iter().any(|node| {
                    node.apparatus_id == target
                        && assigned.contains(node.alternative_group_id.as_str())
                })
                || (unassigned(source)
                    && nodes.iter().any(|node| {
                        node.apparatus_id == target
                            && source_nodes.iter().any(|source| {
                                !source.alternative_group_id.is_empty()
                                    && source.alternative_group_id == node.alternative_group_id
                            })
                    }));
        }
        pechat::pechat_color_stations(to).is_none_or(|colors| {
            pechat::pechat_can_move_order(
                colors,
                map.roll_count,
                map.print_compatibility_width_mm(),
                pechat::pechat_color_stations(from),
            )
        })
    }
}
