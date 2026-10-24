# Xavfsizlik tuzatishlari — 2026-10-24

Foydalanuvchi qarori: auditdagi **1, 3 va 4-bandlar tuzatilsin**;
**2-band keyingi muhokama uchun ochiq qolsin**.

## 1. Bloklangan akkaunt va eski tokenlar

- Sessiya yaratish, o‘qish va yangilashda akkauntning joriy `blocked`/`removed`
  holati tekshiriladi. Keshdagi va diskdan tiklangan tokenlarga ham shu talab qo‘yiladi.
- Akkaunt holatini o‘qib bo‘lmasa, kirishga ruxsat berilmaydi.
- Bloklash, o‘chirish va kirish kodini almashtirish akkauntning barcha rollaridagi
  eski sessiyalarni bekor qiladi. Blokdan chiqarish eski tokenni qayta tiklamaydi.
- Bir vaqtda ishlayotgan sessiya so‘rovi bekor qilingan tokenni keshga yoki
  saqlash joyiga qayta yozib qo‘ymasligi uchun operatsiyalar muvofiqlashtiriladi.
- Chat va omborning ochiq WebSocket ulanishlari voqea yuborishdan oldin va
  20 soniyalik davriy tekshiruvda qayta tekshiriladi. Mavjud ishlab chiqarish
  ulanishlaridagi capability tekshiruvlari ham blok holatini hisobga oladi.

## 3. Mobil API uchun HTTPS

- Asosiy API manzili, login va saqlangan akkaunt manzili HTTPS talab qiladi.
  Avval saqlangan masofaviy HTTP manzili qayta ishlatilmaydi.
- HTTP faqat **debug** rejimida `localhost`, `127.0.0.1`, `::1` uchun ruxsat etiladi.
  Release/profile rejimida API uchun bu istisno yo‘q.
- Avtorizatsiyali so‘rovlar va multipart yuklashlar himoyalangan HTTP klientdan
  foydalanadi. HTTPS → HTTP redirect rad etiladi; boshqa HTTPS origin’ga
  redirectda Authorization/Cookie kabi maxfiy sarlavhalar olib tashlanadi.
- Login va boshqa yozuvchi so‘rovlar avtomatik redirect qilinmaydi.
- Chat yuklash ko‘rsatmasidagi HTTP manzil fayl oqimi o‘qilishidan oldin rad etiladi.
- LAN printerlarning HTTP transporti va Android’dagi unga kerakli ruxsat saqlangan.

## 4. Dependency tuzatishlari

| Paket | Oldin | Hozir |
| --- | --- | --- |
| `rustls` | 0.23.40 | 0.23.46 |
| `ctutils` | 0.4.2 | 0.4.4 |
| `event-listener` | 5.4.1 | 5.4.2 |
| `tokio` | 1.52.3 | 1.53.3 |
| `jsonwebtoken` | 9.3.1 | 10.4.0, `aws_lc_rs` provider |
| `h2` (gateway) | 0.4.15 | 0.4.16 |
| `quinn-proto` (ixtiyoriy HTTP/3) | 0.11.14 | 0.11.18 |

Ishlatilmaydigan SQLx `mysql` feature olib tashlandi. Shuning bilan tuzatilgan
versiyasi yo‘q `rsa 0.9.10` dependency grafigidan chiqdi; backend PostgreSQL ishlatadi.
JWT’da `rust_crypto` provider yoqilmadi, chunki u yana `rsa`ni olib kiradi.
Avvalgi eMudhra tuzatishlari (`webpki-roots 1.0.11`, `rustls-webpki 0.103.16`)
saqlangan; batafsil: [eMudhra deploy qaydi](emudhra-tls-update.md).

2026-10-24 kungi OSV tekshiruvida asosiy `mini_rs_erp` dependency grafigida
qolgan xabar `bincode 1.3.3` qo‘llab-quvvatlanmayotgani haqidagi bildirishnoma.
Bu tuzatish uchun ma’lumot saqlash formati ko‘chirilmadi.

Workspace lockfile’dagi alohida `mini_rs_gateway` uchun `lru 0.16.4`
([RUSTSEC-2026-0253](https://rustsec.org/advisories/RUSTSEC-2026-0253)) va
`protobuf 2.28.0`
([RUSTSEC-2024-0437](https://rustsec.org/advisories/RUSTSEC-2024-0437))
ogohlantirishlari saqlanadi. Ular asosiy backendning default build grafigida yo‘q;
gateway ishlatiladigan deploy uchun alohida moslik tekshiruvi talab qilinadi.
`daemonize`, `derivative`, `rustls-pemfile`, `unsafe-libyaml` uchun ham
qo‘llab-quvvatlash to‘xtaganligi haqidagi xabarlar qolgan. Bu qayd butun
tizimda boshqa zaiflik yo‘qligini bildirmaydi.

## 2. Keyingi muhokama: RPS printer manzili — ochiq

**Hozir tuzatilmagan, foydalanuvchi so‘rovi bilan muhokamaga qoldirilgan.**
`src/rps/client.rs`dagi SSRF bilan bog‘liq xatti-harakat o‘zgartirilmagan.

Printerga chiqarish huquqi bor foydalanuvchi yuborgan `driver_url` orqali backend
ichki HTTP(S) manziliga murojaat qilishi mumkin. Klient redirectlarga ergashadi;
xato javobining matni foydalanuvchiga qaytadi. Oldingi mahalliy tekshiruvda faqat
ikkita loopback stub ishlatilgan: redirect orqali boshqa ichki yo‘lga o‘tish va
javob matnining xatoda qaytishi tasdiqlangan. Production tekshirilmagan.

Muhokama qilinadigan qarorlar:

1. Printer driver qayerda ishlaydi: telefon, LAN, VPN yoki alohida server?
2. Qaysi manzil/portlarga backenddan chiqishga ruxsat bo‘lishi kerak?
3. Driver manzilini faqat admin sozlaydimi; autentifikatsiya qanday bo‘ladi?
4. Redirect, javob hajmi va foydalanuvchiga qaytariladigan xatolar chegarasi.

## Mahalliy tekshiruv natijalari

- `cargo test --locked --lib session_security`: **6 ta test o‘tdi**.
- `cargo test --locked --lib core::session`: **13 ta test o‘tdi**, jumladan
  bekor qilish bilan bir vaqtda kelgan so‘rov va LMDB regressiya tekshiruvlari.
- `tests/security_dependencies.rs`: **2 ta test o‘tdi** — RS256 service-account
  imzosi tekshirildi va noto‘g‘ri turdagi `nbf` claim rad etildi.
- `tests/tls_distrust.rs`: **1 ta test o‘tdi** — odatiy HTTPS ulandi,
  ikkala eMudhra test manzili `CertDistrusted` bilan rad etildi.
- Flutter: endpoint, transport, saqlangan akkaunt, server almashtirish va
  chat yuklash bo‘yicha **8 ta fayldagi 53 ta test o‘tdi**.
- O‘zgargan Dart kodini tekshirishda `flutter analyze` xato topmadi;
  yangi Rust/Dart fayllari format tekshiruvi va ikkala repo `git diff --check` o‘tdi.

JWT/TLS testlari lokal diskda barcha backend binarilarini yana link qilmaslik
uchun Cargo `--locked` orqali yig‘ilgan, dependency fingerprintlari mos
artefaktlarga `rustc --test` bilan ulab ishga tushirildi. Odatdagi muhitda qaytarish:

```bash
cargo test --locked --lib session_security
cargo test --locked --lib core::session
cargo test --locked --test security_dependencies
cargo test --locked --test tls_distrust -- --ignored --nocapture
```

To‘liq release build va production tekshiruvi bu ishda bajarilmagan.

## Deploy qiladigan dasturchiga

Backendni shu kod va `Cargo.lock` bilan `--locked` rejimida qayta build qilib,
yangi binary’ni o‘rnating va ishlayotgan jarayonni yangisiga almashtiring.
Faqat server sertifikatlarini yangilash yoki eski jarayonni restart qilish yetmaydi.
Sog‘lom backendda `tools/runtime/up_domain.sh` build/restartdan oldin qaytadi;
uning o‘zini ishga tushirish yangilanish qo‘llanganini isbotlamaydi.

```bash
cargo build --release --locked \
  --bin mini_rs_erp --bin mini_rs_migrate --bin mini_rs_backup_state
```

Mobil tuzatish foydalanuvchilarga yetishi uchun `accord_mobile_v2`ning yangi
release buildini ham tarqating; API manzili `https://...` bo‘lsin.
Ushbu ish davomida production deploy qilinmagan.
