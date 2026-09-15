fn parse_command(text: &str) -> Option<(String, String)> {
    let mut parts = text.trim().splitn(2, char::is_whitespace);
    let command = parts.next()?.strip_prefix('/')?;
    let command = command.split('@').next()?.to_ascii_lowercase();
    Some((command, parts.next().unwrap_or_default().trim().to_string()))
}

#[derive(Debug, PartialEq, Eq)]
enum InlineLoginInput {
    Code(String),
    Password(String),
}

fn parse_inline_login_input(query: &str) -> Option<InlineLoginInput> {
    let mut parts = query.trim().splitn(2, char::is_whitespace);
    let prefix = parts.next()?.to_ascii_lowercase();
    let value = parts.next()?.trim();
    if value.is_empty() {
        return None;
    }
    match prefix.as_str() {
        "q7" if is_login_code(value) => Some(InlineLoginInput::Code(value.to_string())),
        "p4" => Some(InlineLoginInput::Password(value.to_string())),
        _ => None,
    }
}

fn is_login_code(value: &str) -> bool {
    let value = value.trim();
    matches!(value.len(), 5 | 6) && value.chars().all(|character| character.is_ascii_digit())
}

fn user_account_error_message(error: &TelegramError) -> String {
    match error {
        TelegramError::UserAccountNotConfigured => {
            "User profile ulash sozlanmagan: serverda TELEGRAM_API_ID va TELEGRAM_API_HASH kerak."
                .to_string()
        }
        TelegramError::UserAccountNotAuthorized => {
            "User profile ulanmagan yoki login jarayoni tugamagan.".to_string()
        }
        TelegramError::UserAccountInvalidCode => {
            "Login kodi noto‘g‘ri yoki muddati tugagan. Qayta yuboring.".to_string()
        }
        TelegramError::UserAccountFloodWait { seconds } => {
            let wait = format_wait_duration(*seconds);
            format!(
                "⏳ Telegram kod yuborishni {wait} blokladi (juda ko‘p urinish). Iltimos, qayta-qayta bossangiz blok uzayadi: /cancel qilib {wait} kuting, keyin bir marta urinib ko‘ring."
            )
        }
        TelegramError::UserAccountSendCodeUnavailable => {
            "📵 Telegram bu raqam uchun hozir boshqa yetkazish kanaliga ega emas. Rasmiy Telegram ilovasidagi «Telegram» chatini va Sozlamalar → Qurilmalar (Devices) bo‘limini tekshiring, eski sessiyalarni yoping. Keyin /cancel qilib biroz kutib, qayta urinib ko‘ring. SMS majburan tanlanmaydi."
                .to_string()
        }
        TelegramError::UserAccountResendTooSoon { wait_seconds } => {
            format!(
                "✋ Shoshilmang! Har bosish eski kodni o‘ldiradi. {wait_seconds} soniya kutib, keyin bir marta bosing."
            )
        }
        TelegramError::UserAccountSignUpRequired => {
            "Bu raqam Telegram’da ro‘yxatdan o‘tmagan. Avval Telegram ilovasida account oching."
                .to_string()
        }
        TelegramError::UserAccountAccountMismatch => {
            "Yuborilgan raqam botga kirgan Telegram profilingizga tegishli emas.".to_string()
        }
        TelegramError::UserAccountGroupNotWritable => {
            "Tanlangan guruhga user profile yozolmaydi yoki guruh endi mavjud emas.".to_string()
        }
        TelegramError::UserAccount(error) => format!("User profile ulanmadi: {error}"),
        _ => format!("Telegram user profile xatosi: {error}"),
    }
}

fn format_wait_duration(seconds: u64) -> String {
    if seconds < 90 {
        format!("{seconds} soniyaga")
    } else if seconds < 5400 {
        format!("{} daqiqaga", seconds.div_ceil(60))
    } else {
        format!("{} soatga", seconds.div_ceil(3600))
    }
}

fn telegram_display_name(user: &TelegramUser) -> String {
    let name = format!("{} {}", user.first_name.trim(), user.last_name.trim());
    let name = name.trim();
    if !name.is_empty() {
        name.to_string()
    } else {
        user.username
            .as_deref()
            .unwrap_or("Telegram user")
            .to_string()
    }
}

fn start_error_message(error: &TelegramError) -> &'static str {
    match error {
        TelegramError::InviteExpired => "invite link muddati tugagan",
        TelegramError::InviteAlreadyUsed => "invite link boshqa user tomonidan ishlatilgan",
        TelegramError::InviteNotFound => "invite link topilmadi",
        TelegramError::InviteTokenRequired => "invite token berilmagan",
        _ => "server xatosi",
    }
}

fn general_guide() -> &'static str {
    r#"Assalomu alaykum! Accord botdan foydalanish uchun admin yuborgan invite linkni ochib, /start tugmasini bosing.

📘 Commandlar:
/help — bot imkoniyatlari va role qo‘llanmasini ko‘rsatadi.
/start — invite link orqali tizimga kirish.
/connect — bot turgan guruhni orderlar uchun ulash; bu command guruh ichida yuboriladi."#
}

fn account_guide(account: &TelegramUserAccount) -> String {
    if !account.user_profile_connected {
        return role_guide(account.role);
    }
    let group = match account
        .selected_chat_title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
    {
        Some(title) => format!("Tanlangan guruh: «{title}»."),
        None => "Hali guruh tanlanmagan.".to_string(),
    };
    let order_command = if account.role == TelegramAccountRole::SalesManager {
        "\n/new_order — yangi order yaratish.\n/bot_mode — bot orqali yuborishga o‘tish."
    } else {
        ""
    };
    format!(
        "✅ Siz User profile orqali login qilgansiz.\nSizning rolingiz: {}\nHozirgi yuborish usuli: {}.\n\n{}\n/groups yuboring yoki pastdagi «Guruh tanlash» tugmasini bosing. Tanlangan guruhga orderlar userbot orqali yuboriladi.{}\n/help — qo‘llanmani ko‘rsatish.",
        account.role.label(), account.delivery_mode.label(), group, order_command,
    )
}

fn role_guide(role: TelegramAccountRole) -> String {
    let role_label = role.label();
    let role_details = match role {
        TelegramAccountRole::Admin => {
            r#"🛠 Admin vazifalari:
• Mobile ilovadagi Telegram bo‘limidan bot sozlamalarini boshqarish.
• Admin va sotuv managerlari uchun invite link yaratish.
• Botni order guruhiga qo‘shib, guruhda /connect yuborish."#
        }
        TelegramAccountRole::SalesManager => {
            r#"💼 Sotuv manageri vazifalari:
• Mijozlar va orderlar bilan ishlash.
• /new_order orqali mijoz, mahsulot, holat, qavat/mikron, tiraj va rasmni ketma-ket to‘ldirish.
• Yangi orderlar yuborilgan guruhni kuzatish.
• Order tafsilotlarini ishlab chiqarish jamoasiga yetkazish."#
        }
    };
    let role_commands = match role {
        TelegramAccountRole::Admin => {
            r#"🛠 Admin commandlari:
/help yoki /commands — admin qo‘llanmasi.
/connect — bot turgan guruhni orderlar uchun ulash; guruh ichida yuboriladi."#
        }
        TelegramAccountRole::SalesManager => {
            r#"💼 Sotuv manageri commandlari:
  /new_order — yangi order ochish va tanlangan guruhga yuborish.
  /bot_mode — orderni bot orqali yuborish.
/user_mode — o‘z Telegram profilingiz orqali yuborish va guruh tanlash.
/groups — user profile ulangan bo‘lsa yozish mumkin guruhlarni chiqarish.
/code yoki /password — login kod/parol uchun inline yuborish tugmasini chiqarish.
/cancel — joriy login jarayonini bekor qilish."#
        }
    };
    format!(
        r#"✅ Ulanish muvaffaqiyatli!
Sizning rolingiz: {role_label}

📘 {role_label} qo‘llanmasi
{role_details}

🤖 Commandlar:
/help — {role_label} qo‘llanmasini qayta ko‘rsatadi.
/commands — commandlar ro‘yxatini qayta chiqaradi.
/start — invite link orqali tizimga kirish yoki role ulanishini yangilash.
/connect — guruhni orderlar uchun ulash; guruh ichida yuboriladi.

{role_commands}

📦 Orderlar
Yangi orderlar tanlangan delivery mode bo‘yicha yuboriladi."#
    )
}

fn bot_url(token: &str, method: &str) -> String {
    format!("{TELEGRAM_API_BASE}{token}/{method}")
}

fn truncate_caption(value: &str) -> String {
    value.chars().take(1000).collect()
}

fn first_non_empty(values: &[&str]) -> String {
    values
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .unwrap_or("—")
        .to_string()
}

fn empty_dash(value: &str) -> &str {
    if value.trim().is_empty() {
        "—"
    } else {
        value.trim()
    }
}

fn number_or_dash(value: f64) -> String {
    if !value.is_finite() || value <= 0.0 {
        return "—".to_string();
    }
    if value.fract().abs() < f64::EPSILON {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}
