pub fn returned_paint_image_url(image_id: &str) -> String {
    format!(
        "/v1/mobile/returned-paint/images/view?id={}",
        image_id.trim()
    )
}

pub fn returned_paint_value_count(items: &[ReturnedPaintItem]) -> usize {
    items.iter().map(|item| item.values.len()).sum()
}

pub fn returned_paint_value_count_for_usage(items: &[ReturnedPaintItem], usage: &str) -> usize {
    let usage = usage.trim();
    items
        .iter()
        .filter(|item| item.usage.trim().eq_ignore_ascii_case(usage))
        .map(|item| item.values.len())
        .sum()
}

pub fn returned_paint_has_minimum_values_per_usage(items: &[ReturnedPaintItem]) -> bool {
    let mut rasxot = 0;
    let mut astatka = 0;
    for item in items {
        let count = item.values.len();
        if item.usage.trim().eq_ignore_ascii_case("rasxot") {
            rasxot += count;
        } else if item.usage.trim().eq_ignore_ascii_case("astatka") {
            astatka += count;
        }
        if rasxot >= 3 && astatka >= 3 {
            return true;
        }
    }
    false
}

pub fn returned_paint_report_can_close(items: &[ReturnedPaintItem], has_image: bool) -> bool {
    let mut total = 0;
    let mut rasxot = 0;
    let mut astatka = 0;
    for item in items {
        let count = item.values.len();
        total += count;
        if item.usage.trim().eq_ignore_ascii_case("rasxot") {
            rasxot += count;
        } else if item.usage.trim().eq_ignore_ascii_case("astatka") {
            astatka += count;
        }
    }
    (rasxot >= 3 && astatka >= 3) || (total == 0 && has_image)
}

pub fn completion_report_message(request: &ReturnedPaintRequest) -> String {
    let order_code = request.order_code.trim();
    let order_name = request.order_name.trim();
    let order_id = request.order_id.trim();
    let order_label = match (order_code.is_empty(), order_name.is_empty()) {
        (true, true) => order_id.to_string(),
        (false, true) => format!("{order_code} ({order_id})"),
        (true, false) => format!("{order_name} ({order_id})"),
        (false, false) => format!("{order_code} · {order_name} ({order_id})"),
    };
    match request.status {
        ReturnedPaintStatus::WaitingForBoyoqchiInput => format!(
            "Operator {} {} orderini {} apparatida rasm bilan yopdi. Qaytarilgan bo‘yoq qiymatlari Bo‘yoqchi tomonidan kiritilishi kutilmoqda.",
            request.sender_display_name.trim(),
            order_label,
            request.apparatus.trim(),
        ),
        ReturnedPaintStatus::Completed => format!(
            "Operator {} {} orderini {} apparatida muvaffaqiyatli yopdi. Rasxot bo‘yoq sarfi va Astatka qolgan bo‘yoq miqdorlari, berilgan lak va erituvchi qiymatlari qayd etildi.",
            request.sender_display_name.trim(),
            order_label,
            request.apparatus.trim(),
        ),
    }
}

pub fn returned_paint_astatka_total(
    items: &[ReturnedPaintItem],
) -> Result<f64, ReturnedPaintError> {
    let total = items
        .iter()
        .filter(|item| item.usage.trim().eq_ignore_ascii_case("astatka"))
        .flat_map(|item| item.values.values())
        .try_fold(DecimalAmount::ZERO, |total, value| {
            checked_add(total, DecimalAmount::parse_input(value)?)
        })?;
    total.to_f64()
}

pub fn calculate_returned_paint(
    items: &[ReturnedPaintItem],
) -> Result<ReturnedPaintCalculation, ReturnedPaintError> {
    let mut rasxot = PaintUsageTotals::default();
    let mut astatka = PaintUsageTotals::default();

    for item in items {
        let category = item.category.trim();
        let is_solvent = category.eq_ignore_ascii_case("solvents");
        if !is_solvent && !category.eq_ignore_ascii_case("colors") {
            continue;
        }
        let usage = item.usage.trim();
        let totals = if usage.eq_ignore_ascii_case("rasxot") {
            &mut rasxot
        } else if usage.eq_ignore_ascii_case("astatka") {
            &mut astatka
        } else {
            return Err(ReturnedPaintError::InvalidUsage);
        };
        let is_mix = item.name.trim().eq_ignore_ascii_case("mix");
        for (label, value) in &item.values {
            let value = DecimalAmount::parse_input(value)?;
            if is_solvent {
                totals.direct_alcohol = checked_add(totals.direct_alcohol, value)?;
            } else if is_mix || label.trim().eq_ignore_ascii_case("mix") {
                totals.mix = checked_add(totals.mix, value)?;
            } else {
                totals.direct_paint = checked_add(totals.direct_paint, value)?;
            }
        }
    }

    let rasxot_mix_paint = rasxot.mix.checked_percent(70)?;
    let astatka_mix_paint = astatka.mix.checked_percent(70)?;
    let rasxot_alcohol = checked_add(rasxot.direct_alcohol, rasxot.mix.checked_percent(30)?)?;
    let astatka_alcohol = checked_add(astatka.direct_alcohol, astatka.mix.checked_percent(30)?)?;
    let rasxot_pure_paint = checked_add(rasxot.direct_paint, rasxot_mix_paint)?;
    let astatka_pure_paint = checked_add(astatka.direct_paint, astatka_mix_paint)?;
    let final_used_alcohol = checked_non_negative_sub(rasxot_alcohol, astatka_alcohol)?;
    let final_used_paint = checked_non_negative_sub(rasxot_pure_paint, astatka_pure_paint)?;

    for value in [
        rasxot.mix,
        astatka.mix,
        rasxot_alcohol,
        astatka_alcohol,
        final_used_alcohol,
        rasxot_pure_paint,
        astatka_pure_paint,
        final_used_paint,
    ] {
        validate_storable_decimal(value)?;
    }
    Ok(ReturnedPaintCalculation {
        rasxot_mix_total: rasxot.mix.to_string(),
        astatka_mix_total: astatka.mix.to_string(),
        rasxot_alcohol: rasxot_alcohol.to_string(),
        astatka_alcohol: astatka_alcohol.to_string(),
        final_used_alcohol: final_used_alcohol.to_string(),
        rasxot_pure_paint: rasxot_pure_paint.to_string(),
        astatka_pure_paint: astatka_pure_paint.to_string(),
        final_used_paint: final_used_paint.to_string(),
    })
}

#[derive(Default)]
struct PaintUsageTotals {
    mix: DecimalAmount,
    direct_paint: DecimalAmount,
    direct_alcohol: DecimalAmount,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
struct DecimalAmount(i128);

impl DecimalAmount {
    const SCALE_DIGITS: usize = 12;
    const SCALE_FACTOR: i128 = 1_000_000_000_000;
    const MAX_STORED_UNITS: i128 = 999_999_999_999_999_999 * Self::SCALE_FACTOR;
    const ZERO: Self = Self(0);

    fn parse_input(value: &str) -> Result<Self, ReturnedPaintError> {
        Self::parse(value, 11)
    }

    fn parse_stored(value: &str) -> Result<Self, ReturnedPaintError> {
        Self::parse(value, Self::SCALE_DIGITS)
    }

    fn parse(value: &str, max_fraction_digits: usize) -> Result<Self, ReturnedPaintError> {
        const MAX_SIGNIFICAND_DIGITS: usize = 64;

        let value = value.trim();
        if value.is_empty() || matches!(value.as_bytes().first(), Some(b'-' | b'+')) {
            return Err(ReturnedPaintError::InvalidValue);
        }
        let mut scientific_parts = value.split(['e', 'E']);
        let mantissa = scientific_parts.next().unwrap_or_default();
        let exponent = match scientific_parts.next() {
            Some(value) => value
                .parse::<i32>()
                .map_err(|_| ReturnedPaintError::InvalidValue)?,
            None => 0,
        };
        if scientific_parts.next().is_some() {
            return Err(ReturnedPaintError::InvalidValue);
        }
        let mut parts = mantissa.split('.');
        let integer = parts.next().unwrap_or_default();
        let fraction = parts.next().unwrap_or_default();
        if (integer.is_empty() && fraction.is_empty())
            || parts.next().is_some()
            || (!integer.is_empty() && !integer.bytes().all(|byte| byte.is_ascii_digit()))
            || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(ReturnedPaintError::InvalidValue);
        }
        if integer.len() + fraction.len() > MAX_SIGNIFICAND_DIGITS {
            return Err(ReturnedPaintError::InvalidValue);
        }
        let mut fraction_digits = i64::try_from(fraction.len())
            .ok()
            .and_then(|length| length.checked_sub(i64::from(exponent)))
            .ok_or(ReturnedPaintError::InvalidValue)?;
        let trailing_zero_count = integer
            .bytes()
            .chain(fraction.bytes())
            .rev()
            .take_while(|byte| *byte == b'0')
            .count();
        let removable_zeros = if fraction_digits > 0 {
            trailing_zero_count.min(usize::try_from(fraction_digits).unwrap_or(usize::MAX))
        } else {
            0
        };
        fraction_digits -=
            i64::try_from(removable_zeros).map_err(|_| ReturnedPaintError::InvalidValue)?;
        let significant_len = integer.len() + fraction.len() - removable_zeros;
        let mut significand = 0_i128;
        let mut has_nonzero_digit = false;
        for byte in integer.bytes().chain(fraction.bytes()).take(significant_len) {
            if !has_nonzero_digit && byte == b'0' {
                continue;
            }
            has_nonzero_digit = true;
            significand = significand
                .checked_mul(10)
                .and_then(|value| value.checked_add(i128::from(byte - b'0')))
                .ok_or(ReturnedPaintError::InvalidValue)?;
        }
        if !has_nonzero_digit {
            return Ok(Self::ZERO);
        }
        if fraction_digits > max_fraction_digits as i64 {
            return Err(ReturnedPaintError::InvalidValue);
        }
        let units = if fraction_digits >= 0 {
            let scale_power = i64::try_from(Self::SCALE_DIGITS)
                .ok()
                .and_then(|scale| scale.checked_sub(fraction_digits))
                .and_then(|power| u32::try_from(power).ok())
                .ok_or(ReturnedPaintError::InvalidValue)?;
            significand
                .checked_mul(
                    10_i128
                        .checked_pow(scale_power)
                        .ok_or(ReturnedPaintError::InvalidValue)?,
                )
                .ok_or(ReturnedPaintError::InvalidValue)?
        } else {
            let integer_power = fraction_digits
                .checked_neg()
                .and_then(|power| u32::try_from(power).ok())
                .ok_or(ReturnedPaintError::InvalidValue)?;
            significand
                .checked_mul(
                    10_i128
                        .checked_pow(integer_power)
                        .ok_or(ReturnedPaintError::InvalidValue)?,
                )
                .and_then(|value| value.checked_mul(Self::SCALE_FACTOR))
                .ok_or(ReturnedPaintError::InvalidValue)?
        };
        let amount = Self(units);
        validate_storable_decimal(amount)?;
        Ok(amount)
    }

    fn reuse_string(self, mut value: String) -> String {
        value.clear();
        write!(&mut value, "{self}").expect("writing to String cannot fail");
        value
    }

    fn checked_percent(self, percent: i128) -> Result<Self, ReturnedPaintError> {
        let multiplied = self
            .0
            .checked_mul(percent)
            .ok_or(ReturnedPaintError::InvalidValue)?;
        if multiplied % 100 != 0 {
            return Err(ReturnedPaintError::InvalidValue);
        }
        Ok(Self(multiplied / 100))
    }

    fn to_f64(self) -> Result<f64, ReturnedPaintError> {
        let value = self
            .to_string()
            .parse::<f64>()
            .map_err(|_| ReturnedPaintError::InvalidValue)?;
        if value.is_finite() {
            Ok(value)
        } else {
            Err(ReturnedPaintError::InvalidValue)
        }
    }
}

impl fmt::Display for DecimalAmount {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let integer = self.0 / Self::SCALE_FACTOR;
        let fraction = self.0 % Self::SCALE_FACTOR;
        if fraction == 0 {
            return write!(formatter, "{integer}");
        }
        let fraction = format!("{fraction:0width$}", width = Self::SCALE_DIGITS);
        write!(formatter, "{integer}.{}", fraction.trim_end_matches('0'))
    }
}

fn checked_add(
    left: DecimalAmount,
    right: DecimalAmount,
) -> Result<DecimalAmount, ReturnedPaintError> {
    left.0
        .checked_add(right.0)
        .map(DecimalAmount)
        .ok_or(ReturnedPaintError::InvalidValue)
}

fn checked_non_negative_sub(
    left: DecimalAmount,
    right: DecimalAmount,
) -> Result<DecimalAmount, ReturnedPaintError> {
    if left < right {
        Err(ReturnedPaintError::NegativeFinalValue)
    } else {
        left.0
            .checked_sub(right.0)
            .map(DecimalAmount)
            .ok_or(ReturnedPaintError::InvalidValue)
    }
}

fn validate_storable_decimal(value: DecimalAmount) -> Result<(), ReturnedPaintError> {
    if value < DecimalAmount::ZERO || value.0 > DecimalAmount::MAX_STORED_UNITS {
        Err(ReturnedPaintError::InvalidValue)
    } else {
        Ok(())
    }
}

pub(crate) fn normalize_returned_paint_stored_decimal(
    value: &str,
) -> Result<String, ReturnedPaintError> {
    Ok(DecimalAmount::parse_stored(value)?.to_string())
}

fn required_text(value: String, error: ReturnedPaintError) -> Result<String, ReturnedPaintError> {
    let value = trim_owned(value);
    if value.is_empty() {
        Err(error)
    } else {
        Ok(value)
    }
}

fn normalize_items(
    items: Vec<ReturnedPaintItem>,
) -> Result<Vec<ReturnedPaintItem>, ReturnedPaintError> {
    if items.is_empty() {
        return Err(ReturnedPaintError::MissingItems);
    }
    items
        .into_iter()
        .map(|item| {
            let ReturnedPaintItem {
                usage,
                category,
                name,
                values,
            } = item;
            let usage = lowercase_ascii_owned(usage);
            if !matches!(usage.as_str(), "rasxot" | "astatka") {
                return Err(ReturnedPaintError::InvalidUsage);
            }
            let category = lowercase_ascii_owned(category);
            if !matches!(category.as_str(), "colors" | "lacquers" | "solvents") {
                return Err(ReturnedPaintError::InvalidCategory);
            }
            let name = trim_owned(name);
            if name.is_empty() {
                return Err(ReturnedPaintError::MissingItemName);
            }
            let values = values
                .into_iter()
                .map(|(label, value)| {
                    let label = trim_owned(label);
                    if label.is_empty() {
                        return Err(ReturnedPaintError::InvalidValue);
                    }
                    let amount = DecimalAmount::parse_input(&value)?;
                    Ok((label, amount.reuse_string(value)))
                })
                .collect::<Result<BTreeMap<_, _>, _>>()?;
            if values.is_empty() {
                return Err(ReturnedPaintError::MissingValues);
            }
            Ok(ReturnedPaintItem {
                usage,
                category,
                name,
                values,
            })
        })
        .collect()
}
