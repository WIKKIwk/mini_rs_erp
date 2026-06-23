DOMAIN ?=

.PHONY: up-domain stop-domain

up-domain:
	@./tools/runtime/up_domain.sh "$(DOMAIN)"

stop-domain:
	@./tools/runtime/stop_domain.sh "$(DOMAIN)"
