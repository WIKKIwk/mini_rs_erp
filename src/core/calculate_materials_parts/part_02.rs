
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_density_and_sorts_material_variants() {
        let material = normalize_material(CalculateMaterialUpsert {
            name: " BOPP metal ".to_string(),
            variants: vec![
                CalculateMaterialVariant {
                    micron: 30,
                    coefficient: 1.6,
                    first_layer_coefficient: None,
                    actual_gsm: None,
                },
                CalculateMaterialVariant {
                    micron: 20,
                    coefficient: 1.1,
                    first_layer_coefficient: None,
                    actual_gsm: None,
                },
            ],
            density_g_cm3: 0.91,
            ..CalculateMaterialUpsert::default()
        })
        .expect("valid material");

        assert_eq!(material.name, "BOPP metal");
        assert_eq!(material.variants[0].micron, 20);
        assert!((material.variants[0].coefficient - 1.092).abs() < 0.001);
    }

    #[test]
    fn allows_density_only_material_without_micron_variants() {
        let material = normalize_material(CalculateMaterialUpsert {
            name: "PET custom".to_string(),
            density_g_cm3: 1.4,
            variants: Vec::new(),
            ..CalculateMaterialUpsert::default()
        })
        .expect("density-only material should be valid");

        assert!(material.variants.is_empty());
        assert_eq!(material.density_g_cm3, 1.4);
    }

    #[test]
    fn rejects_legacy_alias_field() {
        let result = serde_json::from_str::<CalculateMaterialUpsert>(
            r#"{
                "name":"PE oq",
                "aliases":["PE white"],
                "density_g_cm3":0.93,
                "variants":[{"micron":30}]
            }"#,
        );

        assert!(result.is_err());
    }

    #[test]
    fn existing_material_with_a_default_name_replaces_the_new_default() {
        let existing = CalculateMaterial {
            id: "existing-pe-oq".to_string(),
            name: "PE oq".to_string(),
            active: true,
            density_g_cm3: 0.93,
            variants: density_variants(&[30], 0.93),
        };

        let materials = merge_default_calculate_materials(vec![existing]);
        let pe_oq = materials
            .iter()
            .filter(|material| normalize_key(&material.name) == "peoq")
            .collect::<Vec<_>>();

        assert_eq!(pe_oq.len(), 1);
        assert_eq!(pe_oq[0].id, "existing-pe-oq");
        assert_eq!(pe_oq[0].density_g_cm3, 0.93);
    }

    #[test]
    fn retired_invented_builtin_is_not_restored_from_an_existing_database() {
        let retired = CalculateMaterial {
            id: "builtin-pe-qora".to_string(),
            name: "PE qora".to_string(),
            active: true,
            density_g_cm3: DEFAULT_PE_DENSITY_G_CM3,
            variants: density_variants(&[30], DEFAULT_PE_DENSITY_G_CM3),
        };

        let materials = merge_default_calculate_materials(vec![retired]);

        assert!(
            !materials
                .iter()
                .any(|material| material.id == "builtin-pe-qora")
        );
    }

    #[tokio::test]
    async fn memory_store_starts_with_exact_materials() {
        let store = MemoryCalculateMaterialStore::new();
        let materials = store.list().await.expect("materials");
        assert!(
            materials
                .iter()
                .any(|material| material.name == "BOPP metal")
        );
        assert!(materials.iter().any(|material| material.name == "PE oq"));
        assert!(materials.iter().any(|material| material.name == "PE PR"));
        assert!(!materials.iter().any(|material| material.name == "PE qora"));
    }

    #[test]
    fn default_pp_gsm_matches_manufacturer_nominal_substance() {
        let materials = default_calculate_materials();
        for name in ["BOPP", "CPP", "MCPP"] {
            let material = materials
                .iter()
                .find(|material| material.name == name)
                .expect("default PP material");
            let variant = material
                .variants
                .iter()
                .find(|variant| variant.micron == 20)
                .expect("20 micron variant");

            let gsm = effective_variant_gsm(material.density_g_cm3, variant)
                .expect("effective default GSM");
            assert!((gsm - 18.1).abs() < 0.001);
        }
    }
}
