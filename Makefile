# Variables for better maintainability
CARGO := cargo
CARGO_NIGHTLY := cargo +nightly
CRATES := . src

.PHONY: lint fmt clean all test

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