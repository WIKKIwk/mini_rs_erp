DOMAIN ?=
MINI_ERP_DEMO_BASE_URL ?= http://127.0.0.1:18081

.PHONY: up-domain stop-domain seed-demo

up-domain:
	@./tools/runtime/up_domain.sh "$(DOMAIN)"

stop-domain:
	@./tools/runtime/stop_domain.sh "$(DOMAIN)"

seed-demo:
	@MINI_ERP_DEMO_BASE_URL="$(MINI_ERP_DEMO_BASE_URL)" python3 tools/demo/seed_demo_data.py
