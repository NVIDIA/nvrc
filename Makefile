# Makefile for NVRC
# Automates common development tasks

.PHONY: help build test clippy fmt clean refresh-pci-ids check-all

# Default target
help:
	@echo "NVRC Makefile Targets:"
	@echo ""
	@echo "  make build              - Build the project"
	@echo "  make test               - Run tests"
	@echo "  make clippy             - Run clippy with -D warnings"
	@echo "  make fmt                - Format code"
	@echo "  make check-all          - Run fmt, clippy, and tests"
	@echo "  make refresh-pci-ids    - Update PCI IDs database (Hopper+ only)"
	@echo "  make clean              - Clean build artifacts"
	@echo ""

# Build targets
build:
	cargo build --release

build-debug:
	cargo build

build-confidential:
	cargo build --release --features confidential

# Test targets
test:
	cargo test

test-all:
	cargo test --all-features

# Code quality
clippy:
	cargo clippy --all-features -- -D warnings

fmt:
	cargo fmt

fmt-check:
	cargo fmt -- --check

# Comprehensive check
check-all: fmt clippy test
	@echo ""
	@echo "All checks passed!"

# PCI IDs database refresh
refresh-pci-ids:
	@echo "Refreshing PCI IDs database (NVIDIA Ampere+, NVSwitch, and Mellanox)..."
	@if [ ! -x ./download_filtered_pci_ids.py ]; then \
		chmod +x ./download_filtered_pci_ids.py; \
	fi
	python3 ./download_filtered_pci_ids.py src/pci_ids_embedded.txt
	@echo ""
	@echo "PCI IDs database updated: src/pci_ids_embedded.txt"
	@echo ""
	@echo "Included generations:"
	@grep -c "^\t23" src/pci_ids_embedded.txt | xargs -I {} echo "    - Hopper (23xx): {} devices"
	@grep -c "^\t2[9a-f]" src/pci_ids_embedded.txt | xargs -I {} echo "    - Blackwell (29xx-2fxx): {} devices"
	@echo ""
	@echo "To verify: make test"

# Show PCI database stats (using Python script for accurate counting)
pci-stats:
	@echo "PCI IDs Database Statistics:"
	@echo ""
	@echo "File: src/pci_ids_embedded.txt"
	@echo "Total lines: $$(wc -l < src/pci_ids_embedded.txt)"
	@echo "Vendors: $$(grep -c '^[0-9a-f][0-9a-f][0-9a-f][0-9a-f]  ' src/pci_ids_embedded.txt)"
	@echo ""
	@echo "NVIDIA Devices (with tab):"
	@echo "    - Ampere (22xx): $$(grep -cE '^	22[0-9a-f][0-9a-f]' src/pci_ids_embedded.txt || echo 0)"
	@echo "    - Hopper (23xx): $$(grep -cE '^	23[0-9a-f][0-9a-f]' src/pci_ids_embedded.txt || echo 0)"
	@echo "    - Blackwell (29xx-2fxx): $$(grep -cE '^	2[9a-f][0-9a-f][0-9a-f]' src/pci_ids_embedded.txt || echo 0)"
	@echo "    - NVSwitch: $$(grep -ci 'nvswitch' src/pci_ids_embedded.txt || echo 0)"
	@echo ""
	@echo "Last updated: $$(stat -c %y src/pci_ids_embedded.txt 2>/dev/null || stat -f %Sm src/pci_ids_embedded.txt)"

# Clean build artifacts
clean:
	cargo clean
	@echo "Build artifacts cleaned"

# Development targets
dev-check:
	@echo "Running quick development checks..."
	@cargo fmt -- --check || (echo "Format check failed. Run: make fmt" && exit 1)
	@cargo clippy -- -D warnings || (echo "Clippy failed" && exit 1)
	@cargo test --lib || (echo "Tests failed" && exit 1)
	@echo "Development checks passed!"

# CI/CD targets
ci: fmt-check clippy test-all
	@echo "CI checks passed!"

# Release build with all checks
release: check-all
	cargo build --release
	cargo build --release --features confidential
	@echo ""
	@echo "Release builds complete!"
	@ls -lh target/release/NVRC

# Documentation
docs:
	cargo doc --no-deps --open

docs-private:
	cargo doc --no-deps --document-private-items --open

