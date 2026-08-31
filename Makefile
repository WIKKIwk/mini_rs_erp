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

.PHONY: verify test-python-tools extract-test-contracts check-generated-test-contracts audit-test-migration up-domain stop-domain seed-demo db-backup db-migrate publish-mobile-apk

verify: check-generated-test-contracts
	@python3 tools/mini_erp_verifier/verify.py

test-python-tools:
	@cd tools/test_migration_audit && uv run --with tree-sitter==0.25.2 --with tree-sitter-rust==0.24.2 python -m unittest -v
	@cd tools/mini_erp_verifier && python3 -m unittest -v

extract-test-contracts:
	@uv run --script tools/test_migration_audit/extract_contracts.py
	@uv run --script tools/test_migration_audit/extract_contracts.py --manifest tools/test_migration_audit/migrations/automatic_http_52843a2.json --output tools/mini_erp_verifier/generated_automatic_contracts.json

check-generated-test-contracts:
	@uv run --script tools/test_migration_audit/extract_contracts.py --check
	@uv run --script tools/test_migration_audit/extract_contracts.py --manifest tools/test_migration_audit/migrations/automatic_http_52843a2.json --output tools/mini_erp_verifier/generated_automatic_contracts.json --check

audit-test-migration:
	@uv run --script tools/test_migration_audit/audit.py

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
