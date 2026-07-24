DOMAIN ?=
MINI_ERP_DEMO_BASE_URL ?= http://127.0.0.1:18081
APK ?=
VERSION_CODE ?=
VERSION_NAME ?=
MINIMUM_VERSION_CODE ?=
MOBILE_RELEASE_DIR ?= data/mobile_releases
MANDATORY_UPDATE ?=
RELEASE_NOTES ?=
RELEASE_NOTES_FILE ?=

.PHONY: up-domain stop-domain seed-demo db-backup db-migrate publish-mobile-apk

up-domain:
	@./tools/runtime/up_domain.sh "$(DOMAIN)"

stop-domain:
	@./tools/runtime/stop_domain.sh "$(DOMAIN)"

seed-demo:
	@MINI_ERP_DEMO_BASE_URL="$(MINI_ERP_DEMO_BASE_URL)" python3 tools/demo/seed_demo_data.py

db-backup:
	@./tools/db/backup_postgres.sh

db-migrate:
	@cargo run --quiet --bin mini_rs_migrate

publish-mobile-apk:
	@./tools/runtime/publish_mobile_apk.sh \
		--apk "$(APK)" \
		--version-code "$(VERSION_CODE)" \
		--version-name "$(VERSION_NAME)" \
		--release-dir "$(MOBILE_RELEASE_DIR)" \
		$(if $(MINIMUM_VERSION_CODE),--minimum-version-code "$(MINIMUM_VERSION_CODE)") \
		$(if $(filter 1 true yes,$(MANDATORY_UPDATE)),--mandatory) \
		$(if $(RELEASE_NOTES_FILE),--notes-file "$(RELEASE_NOTES_FILE)") \
		$(if $(RELEASE_NOTES),--notes "$(RELEASE_NOTES)")
