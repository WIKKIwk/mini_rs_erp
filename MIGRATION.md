# Yopiq WebSocket ulanishlarini ticketga ko‘chirish

Server oltita yo‘lda eski mijozlarga muddatli ruxsat beradi. Flutter V2 har bir ulanish uchun Bearer bilan `POST /v1/mobile/auth/live-ticket` so‘rovini yuboradi. JSON tanasidagi `path` quyidagi jadvaldan olinadi. Server bergan ticket 30 soniya amal qiladi va faqat so‘ralgan yo‘l hamda uni olgan sessiya uchun yaroqli.

| Yo‘l | Oqim |
| --- | --- |
| `/v1/mobile/admin/warehouses/live` | Omborlar |
| `/v1/mobile/admin/system/monitor/live` | Server monitoringi |
| `/v1/mobile/admin/inventory/live` | Inventar |
| `/v1/mobile/admin/production-maps/live` | Ishlab chiqarish xaritalari |
| `/v1/mobile/production-tasks/live` | Qolip va material vazifalari |
| `/v1/mobile/preparation/orders/live` | Tayyorlov buyurtmalari |

Flutter dastlabki va takroriy ulanishlarda `ticket`ni WebSocket manziliga qo‘shadi. Filtrlar, obunalar, protokol va oqim kursori o‘z qiymatini saqlaydi. Iroh’dan WSS’ga o‘tish ham yangi ticket oladi. Upgrade 401 qaytarsa, mijoz yana bir ticket bilan bir marta urinadi. Ikkinchi 401 faol sessiyani tugatadi. Brauzer upgrade holat kodini bermagani uchun ikkinchi yopiq xatodan keyin mijoz ayni ticketni oddiy HTTP GET orqali tekshiradi. Tarmoqdagi uzilishning o‘zi hisobdan chiqarmaydi. Sessiya bekor qilinganda server ochiq soketni ham yopadi. Oqimni qayta ochish ekran yoki omborning mavjud kutish oralig‘i orqali bajariladi.

Chat qayta ulanishda o‘zining bir martalik ticketini oladi. Android yangilanish oqimi ochiq. Avatarni yuklash muvaffaqiyatsiz tugasa, Flutter tegishli profil yoki suhbat API’sidan yangi `avatar_url` oladi va rasmni bir marta qayta yuklaydi.

## Relizda muddatni belgilash

1. Barcha server nusxalari uchun bir xil mutlaq RFC 3339 vaqtini belgilang. Masalan, reliz **2026-11-24 10:00 UTC** da boshlansa, ikki haftalik muddat **2026-12-08 10:00 UTC** da tugaydi.
2. Server jarayonining deploy muhitiga quyidagi satrni kiriting va yangilangan serverlarni odatiy tartibda qayta ishga tushiring. Qiymatni deploy vaqtida bir marta hisoblang va keyingi restartlarda ham aynan shu qiymatni ishlating.

   ```dotenv
   MINI_ERP_LEGACY_WS_QUERY_TOKEN_UNTIL=2026-12-08T10:00:00Z
   ```

3. Shu vaqtgacha oltita yo‘l bitta autentifikatsiya manbasini qabul qiladi — Bearer sarlavhasi, yo‘l uchun ticket yoki eski query-token. Aralash manbalar 401 qaytaradi. Har bir usul bir xil sessiya, rol, capability va ochiq ulanishdagi takroriy tekshiruvlardan o‘tadi.
4. Vaqt chegarasiga yetgan ilk yangi so‘rovdan boshlab eski usul avtomatik 401 qaytaradi. Sozlama berilmasa yoki sanani o‘qib bo‘lmasa ham 401 qaytadi. Muddat allaqachon ochilgan soketni uzmaydi. Uning sessiyasi va huquqlari tekshirilishda davom etadi.
5. Brauzer boshqa origin’dan ulansa, uni `MINI_ERP_CORS_ALLOWED_ORIGINS` ro‘yxatiga kiriting. Tashqi proksi yoki CDN’ning kirish va xato jurnallarida faqat HTTP metodi va yo‘l qolishini tekshiring. Masalan, Nginx formatida yo‘l uchun `$uri` ishlatiladi. To‘liq so‘rov satri, query va Authorization sarlavhasini yozadigan maydonlarni olib tashlang.

## Eski mijozlarni kuzatish

Server muvaffaqiyatli eski upgrade uchun INFO darajasida bitta `legacy_ws_query_connected` hodisasini yozadi. `route` va `role` har doim bor. Mijoz `X-App-Version` yuborsa, versiya `app_version` maydonida chiqadi. Sarlavha sessiya tokenini yoki query parametrlarini takrorlasa, server uning o‘rniga `[redacted]` yozadi. Oddiy Bearer yoki ticket ulanishlari bu hisobga kirmaydi. Bir mijoz qayta ulansa, yana bitta hodisa yoziladi.

Systemd ishlatiladigan serverda quyidagi buyruqlar oqimni ko‘rsatadi va oxirgi 24 soatdagi ulanishlarni sanaydi.

```sh
journalctl -u mini-rs-erp.service -f -o cat | grep -F legacy_ws_query_connected
journalctl -u mini-rs-erp.service --since '24 hours ago' -o cat | grep -Fc legacy_ws_query_connected
```

Barcha nusxalardan kelgan hodisalarni yo‘l, rol va versiya bo‘yicha ko‘ring. Muddatdan keyin bu hisob nolda qoladi. Eski ilova hali urinyaptimi, degan savol uchun ayni yo‘llardagi HTTP 401 sonini ham kuzating. 401 hisobiga yaroqsiz ticket va Bearer bilan qilingan urinishlar ham kiradi.

## Muddatdan keyin eski usulni butunlay olib tashlash

1. Belgilangan mutlaq vaqt o‘tganini tasdiqlang. Har bir instansda eski usul yangi upgrade uchun 401 berishini va yangilangan Flutter V2 ticket bilan ulanishini tekshiring. Tekshiruvlarda sinov hisobidan foydalaning.
2. Deploy muhitlaridan `MINI_ERP_LEGACY_WS_QUERY_TOKEN_UNTIL`ni olib tashlang. Navbatdagi server relizida `src/http/router/mobile/live_auth.rs` ichidagi legacy qabul qilish shoxi va migratsiya hodisasini olib tashlang. Query’da `token` mavjud bo‘lsa darhol 401 qaytaradigan tekshiruvni qoldiring, jumladan yonida yaroqli Bearer yoki ticket bo‘lsa ham.
3. `src/app.rs` va `src/config.rs` dan muddat qiymati va RFC 3339 parserini olib tashlang. Muddatli qabul qilish testlarini doimiy rad etish testlari bilan almashtiring. Oltita yo‘l uchun Bearer/ticket, aralash manbalar, rol, capability, sessiya bekor qilinishi va jurnal maxfiyligi testlarini saqlang. Gateway va HTTP spanlarida query’ni yashirish ticket va avatarlar uchun ham zarur.
4. Serverda `cargo test --workspace` va `cargo clippy --workspace --all-targets`, Flutter’da `flutter analyze` va `flutter test`ni bajaring. Flutter kartalari, omborlar va monitoringda aloqani uzib-qaytarib ko‘ring. Profil rasmini eskirgan URL bilan ham tekshiring.
5. Server nusxalarini ketma-ket yangilang. Eski jarayonlar ulanishlarini yopib tugagach, ilgari ochilgan legacy soketlar ham qolmaydi. Eski mijozga doimiy 401, V2 ga muvaffaqiyatli upgrade kelishini tekshirgach, migratsiya hodisasi uchun vaqtinchalik hisoblagichni olib tashlang va `FIXES.md`da olib tashlangan relizni qayd eting.
