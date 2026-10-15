# ERP sessiyasi, kesish rejasi va tezkor shablon — 2026-10-15

## Dalil chegarasi

Tekshiruv `mini_rs_erp` va `accord_mobile_v2` ning joriy kodida bajarildi. Haqiqiy ishchi bazadagi buyurtmalar, tarixiy requestlar va yo‘qolgan shablonlarning oldingi qiymatlari tekshirilmagan. Quyidagi sabablar koddan va sun’iy lokal fixturelardan tasdiqlandi; ular muayyan tarixiy buyurtmada aynan shu ketma-ketlik bo‘lganini isbotlamaydi.

PostgreSQL tekshiruvlari `/tmp/accord-template-plan-tests` dagi alohida klaster va har bir test uchun yangi bazalarda bajarildi. Ishchi bazaga migratsiya yoki recovery bajarilmadi. Android/iOS qurilmasida ilovani o‘ldirib qayta ishga tushirish, kamera va haqiqiy printer bilan tekshiruv bajarilmadi.

Boshlang‘ich mobil ishchi nusxada Resume readiness bo‘yicha qisman o‘zgarish va regressiya fixturelari allaqachon bor edi. Ular saqlandi; kontraktning yo‘q/malformed flagini rad etish, Rezka fixturelari, qayta ochish va boshqa tekshiruvlar qo‘shildi. Mavjud printer va ulanishni tiklash o‘zgarishlari qayta yozilmadi.

## 1. Paused Rezka ishini davom ettirish

**Alomat → sabab zanjiri:** ekranni qayta ochish lokal scanned inputni yo‘qotadi → `opening_wip_mode` yoki `previous_wip_mode=scan_required` umumiy Start readinessni false qiladi → Resume ham shu readiness bilan disabled bo‘ladi. Scanner esa serverning `resume_input_required=false` kontraktiga binoan yangi inputni qabul qilmaydi va Resume tugmasiga yo‘naltiradi.

**Ildiz sabab:** yangi ishning input talabi va mavjud sessiyaning Resume talabi bir xil deb hisoblangan.

**Tuzatish:** Resume readiness sinxron va ruxsatli server kontrakti ichida alohida hisoblanadi: input qayta talab qilinmasa lokal scan kerak emas; talab qilinsa mos input qabul qilinishi shart. JSONdan kelgan `resume_input_required` haqiqiy boolean ekanligi alohida qayd etiladi. Yo‘q yoki string ko‘rinishidagi flag Resume uchun ruxsat hisoblanmaydi. Start va Merge o‘z input talablarini saqlaydi.

Asosiy kod:

- Mobil `admin_production_map_orders_read_only_helpers.dart`, `admin_production_map_orders_start_card.dart`.
- Kontrakt: `mobile_api_admin_users_workers_models_part_01.dart`, `mobile_api_admin_queue_models_declarations_part_01.dart`.
- Serverning amaldagi session/WIP tekshiruvlari: `src/core/production_map/progress_session/service_progress.rs`, `src/core/production_map/queue/`.

Sun’iy HTTP fixtureda producer outputi, paused consumer sessiyasi, shu sessiyaga tegishli `in_use` input va yana bitta waiting WIP yaratiladi. Resume QR yubormasdan o‘sha session IDni davom ettiradi; input miqdor, og‘irlik va metr qiymatlari saqlanadi, waiting input avtomatik ulanmaydi. Takroriy Resume yangi sessiya yaratmaydi. Shu yo‘l real PostgreSQL store bilan ham bajarildi.

Mobil testlar Rezka/Laminatsiya, input-required true/false, scanner ko‘rsatmasi, majburiy scan, noto‘g‘ri order/stage, band input, material, qolip, permission, busy/frozen, malformed kontrakt, ekranni qayta yaratish va takror bosishni qamrab oladi. Bu qurilmadagi to‘liq app restart testi sifatida talqin qilinmaydi.

## 2. Saqlangan mapdan order ochilmasligi

**Alomat → sabab zanjiri:** saqlangan Rezka rejasi `4 / [1,1,1,1]` o‘zida valid → yangi mahsulot hisobida 5 kadr talab qilinadi → `apply_order_rezka_kadr_count` faqat countni 5 qiladi → guruhlar yig‘indisi 4 bo‘lib qoladi → compiler `rezka_frame_groups_invalid` qaytaradi → mobil decoder aniq sababni umumiy xabarga aylantiradi.

**Ildiz sabab:** bog‘liq count va taqsimot maydonlaridan faqat bittasi transformatsiya qilingan.

**Biznes qoidasini tanlash:** compiler explicit guruhlarning musbatligi va yig‘indisini tekshiradi; runtime bo‘sh guruhlarni implicit reja sifatida ishlata oladi. Yangi avtomatik map generatori standart guruhlar yaratadi, lekin saqlangan tugunda `[1,1,…]` ning avtomatik ekanini isbotlaydigan provenance yo‘q. Shu sabab mavjud explicit reja qayta taqsimlanmaydi.

**Tuzatish:** barcha tegishli canonical Cut tugunlari, alternativlari bilan birga, mutatsiyadan oldin tekshiriladi. Explicit guruhlar yangi talabga mos bo‘lmasa, server quyidagilarni qaytaradi:

```json
{
  "error": "rezka_frame_groups_invalid",
  "message": "Rezka A (cut): kesish rejasida 4 kadr taqsimlangan, mahsulot uchun 5 kadr kerak. Kesish rejasini tekshiring.",
  "cutting_plan_conflicts": [
    {"node_id":"cut", "apparatus_id":"…", "title":"Rezka A", "planned_frames":4, "required_frames":5}
  ]
}
```

Mos `[2,3]` va `[1,2,2]` rejalari saqlanadi. Nol, manfiy, noto‘g‘ri yig‘indi va yig‘indi overflowi rad etiladi. Bo‘sh guruhlarning implicit semantikasi va amaldagi frame rounding qoidasi saqlanadi. `frame_count`, `roll_count`, fizik en va print-val o‘lchami bir-biriga almashtirilmaydi.

Mobil ilova server xabarini, original kod va HTTP statusni saqlaydi. `MobileApiException` request ID va original diagnostika payloadini ham olib yuradi. Noma’lum kod ham endi “server sabab yubormadi” deb yashirilmaydi. Rejani tuzatish yo‘li mavjud: tahrirlash → hisoblash → **Mapni ulash** → xabarda ko‘rsatilgan Rezka tugunining kadr/guruhlarini tuzatish → saqlash/qayta urinish.

Asosiy kod: `src/http/handlers/admin/production_maps_save_helpers.rs`, `production_maps_parts/part_01.rs`, pending/training adapterlari va mobil `mobile_api_admin_production_map_helpers_part_02.dart`. Compiler validatori olib tashlanmagan.

## 3. Boshqa mahsulot tanlanganda A shabloni yo‘qolishi

**Alomat → sabab zanjiri:** A ochiladi → picker SKU/nominigina Bga o‘zgartiradi → draft Aning ID/kodi va map bog‘lanishini saqlaydi → owner+code lookup yoki ID eski yozuvni topadi → upsert A ustidan Bni yozadi → mobil store bir xil IDni almashtiradi. Order ochishdan oldingi alohida quick-save sabab keyingi order validatsiyasi yiqilsa ham bu overwrite allaqachon commit bo‘ladi.

**Ildiz sabab:** mahsulot identifikatsiyasi bilan template identifikatsiyasi aralashgan, hamda order yaratishdan oldin yashirin mustaqil write bajarilgan.

**Tuzatish va kontrakt:**

| Niyat | Natija |
| --- | --- |
| Shu SKU shablonini tahrirlash (`update`) | ID/kod saqlanadi. Ko‘rinadigan nom o‘zgarishi yangi SKU hisoblanmaydi. |
| B yoki mustaqil variant yaratish (`fork`) | ID **va** kod yangilanadi. Server mustaqil design map yaratadi. |
| Shablondan order ochish (`use`) | Alohida order snapshoti yaratiladi; oldindan quick-template upsert yuborilmaydi. |

Picker va yangi katalog mahsulotidan qaytish bitta `picked` yo‘lidan o‘tadi. B uchun mavjud shablon bo‘lsa, ishlatish, mustaqil variant yaratish va bekor qilish tanlovlari alohida saqlanadi. Moslik SKU orqali aniqlanadi, ko‘rinadigan nom orqali emas.

Backend write uchun ID va owner+code yo‘llarini birga tekshiradi. Boshqa SKU/ownerga tegishli identity `409 template_identity_conflict` oladi. Faqat IDni yoki faqat kodni almashtirish buni chetlab o‘tmaydi. Mavjud qoida bo‘yicha admin o‘qishi mumkin bo‘lgan Telegram shablonidan `use` orqali order ochish mumkin; bu shablonni boshqa owner nomidan yangilashga ruxsat bermaydi. PostgreSQLda owner write lock va SQL sharti, SQLiteda `BEGIN IMMEDIATE` hamda SQL sharti alohida store instance’laridagi parallel writelarni ham himoya qiladi.

Fork nodes, edges va custom sozlamalarni saqlaydi; alternative apparatning amaldagi tanlovini va order raqami/miqdorlarini design nusxasiga olib o‘tmaydi. Sessiya, WIP, queue/event tarixi ko‘chirilmaydi. `variant_id` faqat aniq mustaqil variantlarni legacy business dedupedan ajratadi; oddiy template dedupe qoidasi saqlanadi. Eski client variant markerini yubormagan editda ham marker yo‘qolmaydi. Yangi order identitysiga o‘tilganda quick variant metadatasi ajratiladi.

PostgreSQLda order map + design fork + quick template + calculation snapshot + retry receipt bitta transactionda commit bo‘ladi. Faqat design/shablonni saqlash ham bitta transactiondan foydalanadi. Sun’iy trigger bilan quick INSERT kech bosqichda yiqitilib, ikkala yo‘lda ham orphan map/order qolmasligi tekshirildi.

Mobil failed request uchun draft IDni saqlaydi. Server owner + shu ID + payload fingerprint orqali parallel/retryga oldingi commit javobini qaytaradi; shu key bilan boshqa payload `409 order_creation_idempotency_conflict` oladi. `saved_at` fingerprintdan chiqarilgan. TestMode ham alohida validator, rollback, retry cache va parallel request serializationiga ega.

Asosiy kod: `src/core/calculate_orders.rs`, PostgreSQL/SQLite calculate-order storelari, `src/http/handlers/admin/production_maps_template_intent.rs`, `src/db/postgres_mini_order.rs`, mobil kalkulyator picker/order ochish yo‘llari va API adapterlari.

## Bajarilgan tekshiruvlar

Jurnallar lokal `/tmp/accord-template-plan-tests` papkasida. Buyruqlar backend uchun alohida vaqtinchalik CWDdan `--manifest-path` bilan bajarildi; bu runtime storelarni ishchi loyiha ma’lumotlariga yozdirmaydi.

| Tekshiruv | Natija |
| --- | --- |
| Yangi mobil regression filteri: `linked template:`, `without a preliminary`, `resume WIP:` | 35 passed |
| Mavjud `created product is selected when returning to the order` HTTP/UI testi | 1 passed |
| `template_plan_contract`, `--features verification --include-ignored` | 8 passed; uchta test real disposable PostgreSQLdan foydalanadi |
| SQLite legacy calculate-order testlari | 7 passed; qonuniy business dedupe ham tekshirildi |
| Eski backend queue-progress to‘plami | 44 passed, 1 failed |
| Eski backend production-map/order to‘plami | 20 passed, 1 failed |
| Tanlangan Dart fayllarini analyze qilish | Error/warning yo‘q; bitta avvalgi `unnecessary_import` info |
| `cargo check --features verification --tests` | Bajarildi; mavjud warninglar bor |

Keng to‘plamlar to‘liq yashil deb e’lon qilinmaydi. Alohida HEAD nusxasida ikkala backend failure ham qayta tekshirildi: `laminatsiya_queue_exposes_start_for_waiting_print_wip_behind_unready_queue_head` va `production_map_order_number_is_immutable_on_update` avvaldan yiqiladi. Kalkulyatorning to‘liq mobil to‘plami HEADda **17 passed / 27 failed**, yakuniy runida **25 passed / 26 failed**. Eski mobil xatolar orasida eskirgan UI matnlari, viewport/layout va canonical fixture talablari bor. Ular bu taskda umumiy tozalashga aylantirilmadi.

Tuzatishdan oldingi lokal dalillar: Resume tugmasi `onPressed=null`; boshqa SKU tanlashda eski template ID saqlanishi; backendning faqat `{error:rezka_frame_groups_invalid}` javobi; SQLite va PostgreSQL upsertining A yozuvini B bilan almashtirishi. Ularning jurnallari `mobile-resume-before.log`, `mobile-identity-before.log`, `backend-before.log`, `postgres-before.log` da.

Qayta bajarishning asosiy buyruqlari:

```sh
# disposable PostgreSQL klasteri: mini_rs_erp va mini_rs_erp_owner rollari,
# test runner uchun CREATE DATABASE huquqi kerak.
TEMPLATE_PLAN_TEST_DATABASE_URL=postgresql://TEST_ROLE@127.0.0.1:TEST_PORT/postgres \
  cargo test --manifest-path /PATH/mini_rs_erp/Cargo.toml \
  --features verification --test template_plan_contract -- --include-ignored

# accord_mobile_v2 ichidan:
flutter test --no-pub test/admin_calculate_screen_test.dart \
  test/admin_production_map_test_screen_test.dart \
  --name 'linked template:|without a preliminary|resume WIP:'
```

## Migratsiya va alohida recovery rejasi

`0145_order_creation_receipts.sql` qo‘shildi. Backendni chiqarishdan oldin odatdagi migratsiya jarayoni orqali qo‘llash kerak. Bu migratsiya eski shablonlarni taxminan qayta yaratmaydi.

Tarixiy A shabloni haqiqatda overwrite bo‘lgan bo‘lsa:

1. Hozirgi B shabloni/mapini va bog‘lanishlarini eksport qilib saqlash; Bni A deb qayta nomlab ustidan yozmaslik.
2. Aning **SKU va owner**ini ishonchli buyurtma snapshoti, audit/backuplar yoki avvalgi exportdan aniqlash. Faqat nomga yoki hozirgi map title’iga tayanmaslik.
3. `mini_orders.calculation_json` va shu orderning saqlangan production mapidan hisob-kitob, layers, fysisk o‘lchamlar, print parametrlari va custom kesish rejasini ajratib olish. Mapning o‘zi yo‘qolgan customer/material/calculation qiymatlarini taxmin qilish uchun yetarli dalil emas.
4. Yangi template ID/kod va mustaqil design map identitysi bilan A variantini tayyorlash. Bda band bo‘lgan eski kodni qayta ishlatmaslik; order/session/WIP tarixini design mapga ko‘chirmaslik.
5. Barcha Cut tugunlarining explicit guruhlari va frame talabini, en/val/rang farqini, layers va owner bog‘lanishini tekshirish. Mos kelmagan custom rejani operator tasdiqlamaguncha saqlamaslik.
6. Alohida transactionda tiklash; serverdan force reload qilib A va Bning ikkalasini, map mustaqilligini va eski order snapshotlari o‘zgarmaganini tekshirish. Recovery retry uchun bir xil mustaqil identitylardan foydalanish.

Bu recovery uchun real manba dalillari va alohida bajarish so‘rovi kerak; kod tuzatish jarayonida tarixiy ma’lumot avtomatik o‘zgartirilmadi.
