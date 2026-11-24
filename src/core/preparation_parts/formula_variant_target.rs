/// A recipe in the catalog, or in the exact order/material context.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormulaVariantTarget {
    #[serde(default)]
    pub order_id: String,
    pub product_code: String,
    pub material_id: String,
    pub variant_id: String,
    #[serde(default)]
    pub recipe_key: String,
}

pub fn normalize_variant_name(value: &str) -> Result<String, PreparationError> {
    let name = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() || name.chars().count() > 160 {
        return Err(PreparationError::Invalid(
            "Variant nomi 1–160 ta belgidan iborat bo‘lishi kerak",
        ));
    }
    Ok(name)
}

impl FormulaVariantReuse {
    pub fn validate(&self) -> Result<(), PreparationError> {
        for (value, limit) in [
            (&self.product_code, 160),
            (&self.material_id, 128),
            (&self.source_variant_id, 160),
        ] {
            if value.trim().is_empty() || value.trim().chars().count() > limit {
                return Err(PreparationError::Invalid("Variant ma’lumotlari noto‘g‘ri"));
            }
        }
        Ok(())
    }
}

impl FormulaVariantCreate {
    pub fn normalized_name(&self) -> Result<String, PreparationError> {
        if self.product_code.trim().is_empty() || self.product_code.trim().chars().count() > 160
            || self.material_id.trim().is_empty() || self.material_id.trim().chars().count() > 128
            || self.request_id.trim().is_empty() || self.request_id.trim().chars().count() > 160
        {
            return Err(PreparationError::Invalid("Variant ma’lumotlari noto‘g‘ri"));
        }
        normalize_variant_name(&self.name)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormulaShare {
    pub name: String,
    pub percent: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormulaSharesUpdate {
    pub product_code: String,
    pub material_id: String,
    pub shares: Vec<FormulaShare>,
}

impl FormulaSharesUpdate {
    pub fn normalized(&self) -> Result<Vec<(String, i64)>, PreparationError> {
        if self.product_code.trim().is_empty() || self.product_code.trim().chars().count() > 160
            || self.material_id.trim().chars().count() > 128
        {
            return Err(PreparationError::Invalid("Mahsulot yoki homashyo noto‘g‘ri"));
        }
        if self.shares.is_empty() || self.shares.len() > 100 {
            return Err(PreparationError::Invalid("1–100 ta shnek formulasi bo‘lishi kerak"));
        }
        let mut shares = BTreeMap::new();
        let mut total = 0;
        for share in &self.shares {
            let name = share.name.trim();
            if name.is_empty() || name.chars().count() > 80 || shares.contains_key(name) {
                return Err(PreparationError::Invalid("Shnek formulasi bo‘sh yoki takrorlangan"));
            }
            let percent = decimal(&share.percent)?;
            if percent > 100 * SCALE {
                return Err(PreparationError::Invalid("Shnek ulushi 100% dan oshmasligi kerak"));
            }
            total += percent;
            shares.insert(name.to_owned(), percent);
        }
        if total != 100 * SCALE {
            return Err(PreparationError::Invalid("Shnek ulushlari jami 100% bo‘lishi kerak"));
        }
        Ok(shares.into_iter().collect())
    }
}

impl FormulaSetRename {
    pub fn normalized(&self) -> Result<(&str, &str), PreparationError> {
        let id = self.set_id.trim();
        let name = self.name.trim();
        if id.len() != 12 || !id.starts_with("9502") || !id.bytes().all(|c| c.is_ascii_digit()) {
            return Err(PreparationError::Invalid("Formula komplekti kodi noto‘g‘ri"));
        }
        if name.is_empty() || name.chars().count() > 160 {
            return Err(PreparationError::Invalid(
                "Komplekt nomi 1–160 ta belgidan iborat bo‘lishi kerak",
            ));
        }
        Ok((id, name))
    }
}

fn default_formula_name() -> String {
    "Asosiy".to_string()
}

impl FormulaUpsert {
    pub fn product_key(&self) -> Result<String, PreparationError> {
        let code = self.product_code.trim().to_string();
        if code.is_empty() || code.chars().count() > 160 {
            return Err(PreparationError::Invalid("Mahsulot kodi noto‘g‘ri"));
        }
        Ok(code)
    }

    pub fn formula_name(&self) -> Result<String, PreparationError> {
        let name = self.name.trim().to_string();
        if name.is_empty() || name.chars().count() > 80 {
            return Err(PreparationError::Invalid(
                "Formula nomi 1–80 ta belgidan iborat bo‘lishi kerak",
            ));
        }
        Ok(name)
    }

    /// Formula qaysi homashyo (calculate-material oilasi) uchun.
    /// Micron scope kalitiga kirmaydi.
    pub fn material_key(&self) -> Result<String, PreparationError> {
        let id = self.material_id.trim().to_string();
        if id.is_empty() || id.chars().count() > 128 {
            return Err(PreparationError::Invalid("Homashyo tanlanmadi"));
        }
        Ok(id)
    }

    /// Validated (item_code, percent) pairs sorted alphabetically by code.
    /// Display order by name is applied by the caller once names resolve.
    /// Safety rule: percentages must total exactly 100%.
    pub fn normalized_lines(&self) -> Result<Vec<(String, i64)>, PreparationError> {
        if self.lines.is_empty() || self.lines.len() > 100 {
            return Err(PreparationError::Invalid("1–100 ta seriyo tanlang"));
        }
        let mut seen = BTreeSet::new();
        let mut result = Vec::new();
        let mut total: i64 = 0;
        for line in &self.lines {
            let code = line.item_code.trim();
            if code.is_empty() || !seen.insert(code.to_string()) {
                return Err(PreparationError::Invalid(
                    "Seriyo bo‘sh yoki takrorlangan",
                ));
            }
            let percent = decimal(&line.percent)?;
            if percent > 100 * SCALE {
                return Err(PreparationError::Invalid(
                    "Foiz 100 dan oshmasligi kerak",
                ));
            }
            total = total.checked_add(percent).ok_or(PreparationError::Invalid(
                "Jami foiz hisoblanmadi",
            ))?;
            result.push((code.to_string(), percent));
        }
        if total != 100 * SCALE {
            return Err(PreparationError::Invalid(
                "Jami foiz 100% bo‘lishi kerak",
            ));
        }
        result.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(result)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialResponsibilityAssign {
    pub principal_ref: String,
    pub material_id: String,
}

impl MaterialResponsibilityAssign {
    pub fn principal_key(&self) -> Result<String, PreparationError> {
        let ref_ = self.principal_ref.trim().to_string();
        if ref_.is_empty() || ref_.chars().count() > 128 {
            return Err(PreparationError::Invalid("Foydalanuvchi topilmadi"));
        }
        Ok(ref_)
    }

    pub fn material_key(&self) -> Result<String, PreparationError> {
        let id = self.material_id.trim().to_string();
        if id.is_empty() || id.chars().count() > 128 {
            return Err(PreparationError::Invalid("Homashyo noto‘g‘ri"));
        }
        Ok(id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialResponsibilityDelete {
    pub principal_ref: String,
    pub material_id: String,
}

impl MaterialResponsibilityDelete {
    pub fn principal_key(&self) -> Result<String, PreparationError> {
        let ref_ = self.principal_ref.trim().to_string();
        if ref_.is_empty() || ref_.chars().count() > 128 {
            return Err(PreparationError::Invalid("Foydalanuvchi topilmadi"));
        }
        Ok(ref_)
    }

    pub fn material_key(&self) -> Result<String, PreparationError> {
        let id = self.material_id.trim().to_string();
        if id.is_empty() || id.chars().count() > 128 {
            return Err(PreparationError::Invalid("Homashyo noto‘g‘ri"));
        }
        Ok(id)
    }
}
