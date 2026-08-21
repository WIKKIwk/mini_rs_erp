
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::apparatus_standard::{
        ApparatusDisplay, CanonicalApparatusPatch, CanonicalCommandMetadata, ProcessTechnology,
        test_support::{TestApparatusSpec, canonical_draft},
    };

    async fn seeded_apparatus(entries: &[(&str, &str)]) -> CanonicalApparatusService {
        let apparatus = CanonicalApparatusService::memory();
        for (id, name) in entries {
            apparatus
                .seed_for_test(
                    ApparatusId::new((*id).to_string()).expect("test apparatus id"),
                    canonical_draft(&TestApparatusSpec::print(
                        id,
                        name,
                        ProcessTechnology::Rotogravure,
                        Some(7),
                    )),
                )
                .await
                .expect("seed canonical apparatus");
        }
        apparatus
    }

    async fn service() -> FactoryLocationService {
        FactoryLocationService::new(
            Arc::new(MemoryFactoryLocationStore::new()),
            seeded_apparatus(&[
                ("apparatus:default:bosma_7", "7 ta rangli bosma aparat"),
                ("apparatus:default:asset-010", "Rezka"),
            ])
            .await,
        )
    }

    #[tokio::test]
    async fn creates_unique_immutable_id_and_resolves_apparatus_by_id() {
        let service = service().await;
        let created = service
            .create(FactoryLocationCreate {
                name: " Bosma oldi ".to_string(),
                apparatus_ids: vec!["apparatus:default:bosma_7".to_string()],
            })
            .await
            .expect("create state");
        assert!(created.id.starts_with("state_"));
        assert_eq!(created.id.len(), "state_".len() + 32);
        assert_eq!(created.name, "Bosma oldi");
        assert_eq!(created.apparatus.len(), 1);

        let inactive = service
            .update(
                &created.id,
                FactoryLocationUpdate {
                    active: Some(false),
                    ..Default::default()
                },
            )
            .await
            .expect("deactivate location");
        assert!(!inactive.active);

        let updated = service
            .replace_apparatus(
                &created.id,
                FactoryLocationApparatusReplace {
                    apparatus_ids: vec!["apparatus:default:asset-010".to_string()],
                },
            )
            .await
            .expect("replace apparatus");
        assert_eq!(updated.id, created.id);
        assert_eq!(updated.name, created.name);
        assert!(!updated.active);
        assert_eq!(
            updated.apparatus[0].id.as_str(),
            "apparatus:default:asset-010"
        );
        assert_eq!(updated.apparatus[0].name, "Rezka");
    }

    #[tokio::test]
    async fn renaming_apparatus_display_name_does_not_change_placement_id() {
        let apparatus = seeded_apparatus(&[(
            "apparatus:custom:placement-rename-proof",
            "Original display",
        )])
        .await;
        let service = FactoryLocationService::new(
            Arc::new(MemoryFactoryLocationStore::new()),
            apparatus.clone(),
        );
        let created = service
            .create(FactoryLocationCreate {
                name: "Rename proof".to_string(),
                apparatus_ids: vec!["apparatus:custom:placement-rename-proof".to_string()],
            })
            .await
            .expect("create placement");

        apparatus
            .patch(
                ApparatusId::new("apparatus:custom:placement-rename-proof".to_string()).unwrap(),
                1,
                CanonicalApparatusPatch {
                    display: Some(ApparatusDisplay {
                        display_name: "Renamed display".to_string(),
                        description: "Renamed canonical fixture".to_string(),
                        catalog_order: 1,
                    }),
                    ..Default::default()
                },
                CanonicalCommandMetadata::new("user:test", "command:test:rename-placement"),
            )
            .await
            .expect("rename apparatus");
        let updated = service
            .replace_apparatus(
                &created.id,
                FactoryLocationApparatusReplace {
                    apparatus_ids: vec!["apparatus:custom:placement-rename-proof".to_string()],
                },
            )
            .await
            .expect("re-resolve placement");

        assert_eq!(updated.apparatus[0].id, created.apparatus[0].id);
        assert_eq!(updated.apparatus[0].name, "Renamed display");
    }

    #[tokio::test]
    async fn rejects_display_names_and_legacy_title_ids_as_placement_keys() {
        let service = service().await;
        for apparatus_id in [
            "Rezka",
            "apparatus:Rezka",
            "apparatus:missing",
            "apparatus:custom:rezka",
        ] {
            assert_eq!(
                service
                    .create(FactoryLocationCreate {
                        name: format!("Invalid {apparatus_id}"),
                        apparatus_ids: vec![apparatus_id.to_string()],
                    })
                    .await,
                Err(FactoryLocationError::InvalidApparatus),
                "placement must not resolve apparatus by display title: {apparatus_id}"
            );
        }
    }

    #[tokio::test]
    async fn rejects_duplicate_names_and_unknown_apparatus() {
        let service = service().await;
        service
            .create(FactoryLocationCreate {
                name: "Laminat oldi".to_string(),
                apparatus_ids: Vec::new(),
            })
            .await
            .expect("create state");
        assert_eq!(
            service
                .create(FactoryLocationCreate {
                    name: " laminat OLDI ".to_string(),
                    apparatus_ids: Vec::new(),
                })
                .await,
            Err(FactoryLocationError::DuplicateName)
        );
        assert_eq!(
            service
                .create(FactoryLocationCreate {
                    name: "Noma'lum".to_string(),
                    apparatus_ids: vec!["apparatus:missing".to_string()],
                })
                .await,
            Err(FactoryLocationError::InvalidApparatus)
        );
    }

    #[tokio::test]
    async fn retired_apparatus_cannot_receive_new_placement() {
        let apparatus = seeded_apparatus(&[(
            "apparatus:custom:retired-placement",
            "Retired placement display",
        )])
        .await;
        apparatus
            .retire(
                ApparatusId::new("apparatus:custom:retired-placement".to_string()).unwrap(),
                1,
                "decommissioned".to_string(),
                CanonicalCommandMetadata::new("user:test", "command:test:retire-placement"),
            )
            .await
            .expect("retire canonical apparatus");
        let service =
            FactoryLocationService::new(Arc::new(MemoryFactoryLocationStore::new()), apparatus);

        assert_eq!(
            service
                .create(FactoryLocationCreate {
                    name: "Retired placement".to_string(),
                    apparatus_ids: vec!["apparatus:custom:retired-placement".to_string()],
                })
                .await,
            Err(FactoryLocationError::InvalidApparatus)
        );
    }
}
