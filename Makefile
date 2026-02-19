# Variables for better maintainability
CARGO := cargo
CARGO_NIGHTLY := cargo +nightly
CRATES := . src

.PHONY: lint fmt clean all test


################################################################################
# Reusable Functions
################################################################################

# Check if a required tool is installed
# Usage: $(call require-tool,tool-name,install-hint)
define require-tool
@which $(1) >/dev/null 2>&1 || (echo "Error: $(1) not installed. $(2)" && exit 1)
endef


# Default target
all: lint fmt

# Run lint checks across all directories
lint:
	@echo "Running clippy on all crates..."
	@for dir in $(CRATES); do \
		echo "Running clippy in $$dir"; \
		(cd $$dir && $(CARGO_NIGHTLY) clippy); \
	done

# Format code across all directories
fmt:
	@echo "Formatting code in all crates..."
	@for dir in $(CRATES); do \
		echo "Formatting in $$dir"; \
		(cd $$dir && $(CARGO_NIGHTLY) fmt); \
	done

# Clean all build artifacts
clean:
	@echo "Cleaning build artifacts..."
	for dir in $(CRATES); do \
		echo "Cleaning in $$dir"; \
		(cd $$dir && $(CARGO) clean); \
	done

# Run tests across all directories
test:
	@echo "Running tests on all directories..."
	@for dir in $(CRATES); do \
		echo "Running test in $$dir"; \
		(cd $$dir && $(CARGO) test); \
	done

# Check if you're using the recommended Rust version
check-rust-version:
	@echo "Checking Rust version..."
	@rustc --version

################################################################################
# Changelog Management Targets (Knope)
################################################################################

check-knope: ## Verify Knope is installed
	$(call require-tool,knope,Install with: cargo install knope)

changelog.add: check-knope ## Create a new changelog fragment interactively
	knope document-change

changelog.prepare: check-knope ## Preview the next release changelog
	knope prepare-release

changelog.release: check-knope ## Prepare and create a new release [version=X.Y.Z]
ifndef version
	$(error version is required. Usage: make changelog.release version=0.9.0)
endif
	knope release

changelog.list: ## List all pending changelog fragments
	@echo "Pending changelog fragments:"
	@find .changeset -name '*.md' -not -name 'README.md' -exec basename {} \; | sort

