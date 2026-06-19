REGISTRY := local
.DEFAULT_GOAL := eif

.PHONY: check build test clean eif run run-debug run-local stop logs status

out:
	mkdir -p out

# ── Primary target: build the EIF and PCRs ───────────────────────────────────
# Requires a sibling coordinator checkout — see Containerfile header.
#   COORDINATOR_DIR defaults to the local dev layout's sibling path.
COORDINATOR_DIR ?= ../coordinator
eif: out
	docker buildx build \
		--tag $(REGISTRY)/chat-relayer \
		--progress=plain \
		--platform linux/amd64 \
		--build-context coordinator=$(COORDINATOR_DIR) \
		--output type=local,rewrite-timestamp=true,dest=out \
		-f Containerfile \
		.

# ── Local dev ─────────────────────────────────────────────────────────────────
check:
	cd src/relayer && cargo check

build:
	cd src/relayer && cargo build

test:
	cd src/relayer && cargo test

run-local:
	cd src/relayer && cargo run

# ── Enclave management (EC2 only) ─────────────────────────────────────────────
run: out/chat-relayer.eif
	sudo nitro-cli \
		run-enclave \
		--cpu-count 2 \
		--memory 2048 \
		--eif-path out/chat-relayer.eif
	@echo ""
	@echo "Enclave running. Start host bridges:"
	@echo "  ./parent_forwarder.sh"
	@echo ""
	@echo "Smoke test:"
	@echo "  curl http://localhost:4002/health"

run-debug: out/chat-relayer.eif
	sudo nitro-cli \
		run-enclave \
		--cpu-count 2 \
		--memory 2048 \
		--eif-path out/chat-relayer.eif \
		--debug-mode \
		--attach-console

stop:
	sudo nitro-cli terminate-enclave --all

logs:
	sudo nitro-cli console --enclave-name \
		$$(sudo nitro-cli describe-enclaves | jq -r '.[0].EnclaveID')

status:
	@echo "=== ENCLAVE STATUS ==="
	sudo nitro-cli describe-enclaves 2>/dev/null || echo "No enclaves running"

run-host:
	./parent_forwarder.sh

clean:
	rm -rf out
	cd src/relayer && cargo clean
