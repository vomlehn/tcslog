# Makefile for automated Rust project creation with Claude Code

.PHONY: all setup build test clean install help

# Project variables
PROJECT_NAME := task-manager
SIM_NAME := simulator
SRC_DIR := src
DOCS_DIR := docs
PROMPTS_DIR := prompts

TCSLOG=$(DOCS_DIR)/tcslog.rst
TCSPECIAL = .
RUST = .

TCSLOG_CODE = tcslog
TCSLOG_TEST = 
TCSLOG_RUST = 
TCSLOG_TAR = $(TCSLOG_RUST).tar.gz
TCSLOG_OUTPUT = compressed tar file $(TCSLOG_TAR)
PROMPT = Generate Rust code ($(TCSLOG_CODE)) and create $(TCSLOG_OUTPUT) from $(TCSLOG)

TCSLOG_CRATES = tcslib tcslibgs g tcsmoc tcssim tcspayload.json

# Uid regular expression definitions
TS_RE_ATOM = [0-9a-f]
TS_RE_CHUNK = ${TS_RE_ATOM}${TS_RE_ATOM}${TS_RE_ATOM}${TS_RE_ATOM}
TS_RE_SECS = ${TS_RE_CHUNK}_${TS_RE_CHUNK}_${TS_RE_CHUNK}_${TS_RE_CHUNK}
TS_RE_MSECS = ${TS_RE_CHUNK}_${TS_RE_CHUNK}
TS_RE = ${TS_RE_SECS}_${TS_RE_MSECS}

# Output directory for tcslog-sample
# TODO: pass to tcslog-sample as an argument
TCSLOG_SAMPLE_DIR = /tmp/tcslog-sample

RELEASE = --release
RELEASE =

FIXUP = set -x; \
		echo "Project fixup..."; \
		sed -i 's/into_raw_fd/as_raw_fd/g' g/src/endpoint.rs; \
		sed -i 's/into_raw_fd/as_raw_fd/g' g/src/dh.rs;
FIXUP =

FIXUP_TEST =
FIXUP_SIM =

# Default target
all: build test

# Display help
help:
	@echo "Makefile for Rust Project with Claude Code"
	@echo ""
	@echo "Usage:"
	@echo "  make all         - Generate, build, and test the project"
	@echo "  make generate    - Use Claude Code to generate project files"
	@echo "  make build       - Build the Rust project"
	@echo "  make test        - Run all tests"
	@echo "  make run         - Run the application"
	@echo "  make clean       - Remove build artifacts"
	@echo "  make install     - Install the binary globally"
	@echo "  make setup       - Initial setup (create directories)"

# Create necessary directories
setup:
	@echo "Setting up project structure..."
	@mkdir -p $(DOCS_DIR) $(PROMPTS_DIR)
	@echo "✓ Directories created"

# Generate project using Claude Code
generate: $(TCSLOG)
	( \
		set -eu; \
		echo "Generating project with Claude Code..."; \
		if [ ! -f "$(TCSLOG)" ]; then \
			echo "Error: $(TCSLOG) not found"; \
			exit 1; \
		fi \
	) 2>&1 | tee generate.out
	( \
		set -x; \
		set -eu; \
		start_time=$$(date +"%s"); \
		claude -p \
		    "$(PROMPT)" \
		   --allowedTools Read,Write,Edit,MultiEdit \
		    --verbose; \
		print-elapsed $$start_time; \
		echo "✓ Project files generated" \
	) 2>&1 | tee -a generate.out

# Alternative: Use echo to pipe commands
generate-alt:
	@echo "Generating project with Claude Code (alternative method)..."
	@echo "Read docs/design.rst and create a complete Rust project with:\n\
	1. Cargo.toml with dependencies (serde, serde_json, chrono, clap)\n\
	2. All source files: main.rs, task.rs, storage.rs, cli.rs\n\
	3. Unit tests in each module\n\
	4. Integration tests\n\
	5. README.md and .gitignore\n\
	Generate all files without confirmation." | claude --model claude-sonnet-4-5-20250929

# Build the project
build:
	( \
		set -eu; \
		$(FIXUP) \
		echo "Building the project..."; \
		cd $(RUST) && cargo build $(RELEASE) --bin tcspecial; \
		echo "✓ Build complete" \
	) 2>&1 | tee build.out

# Run tests
test:
	( \
		set -eu; \
		$(FIXUP_TEST) \
		echo "Running tests..."; \
		cd $(RUST) && cargo test; \
		echo "✓ Tests complete"; \
	)

# Run the tcspecial application
run:
	( \
		set -eu; \
		$(FIXUP) \
		echo "Running $(PROJECT_NAME)..."; \
		cd $(RUST) && RUST_LOG=info cargo run --bin tcspecial \
	)

.PHONY: tcslog-sample
tcslog-sample:
	cd tcslog-sample && cargo run --bin tcslog-sample -- prefix_ _suffix 1

.PHONY: tcslog-dump
tcslog-dump:
	#cd tcslog-dump && cargo run --bin tcslog-dump -- prefix_ _suffix
	set -eu; \
		infile="$$(ls $(TCSLOG_SAMPLE_DIR)/ | sed -e '2,$$d')"; \
		Uid="$$(echo "$$infile" | \
			sed -e s/^prefix_// -e s/_suffix$$//)"; \
		echo infile $$infile; \
		echo Uid $$Uid; \
		cd tcslog-dump; \
		cargo run --bin tcslog-dump -- prefix_ "$$Uid" _suffix

# Clean build artifacts
clean:
	@echo "Cleaning build artifacts..."
	-cargo clean
	rm -f generate.out build.out run.out test.out $(TCSLOG_TAR)
	rm -rf $(TCSLOG_RUST) $(TCSLOG_TAR)
	@echo "✓ Clean complete"


# Clean everything including generated source
distclean: clean
	@echo "Removing all generated files..."
	rm -f Cargo.lock Cargo.toml
	rm -rf $(TCSLOG_CRATES)
	rm -rf target
	@echo "✓ Project reset"

# Install binary globally
install: build
	@echo "Installing $(PROJECT_NAME)..."
	cd $(RUST) && cargo install --path .
	@echo "✓ Installed to ~/.cargo/bin/$(PROJECT_NAME)"

# Check code quality
check:
	@echo "Running cargo check..."
	cd $(RUST) && cargo check
	cd $(RUST) && cargo clippy -- -D warnings
	cd $(RUST) && cargo fmt -- --check

# Format code
format:
	cd $(RUST) && cargo fmt

# Create release build
release: test
	@echo "Creating release build..."
	cd $(RUST) && cargo build --release
	@echo "✓ Release binary: target/release/$(PROJECT_NAME)"

# Run with example data
demo: build
	@echo "Running demo..."
	cd $(RUST) && cargo run -- add "Buy groceries" --desc "Milk, eggs, bread"
	cd $(RUST) && cargo run -- add "Write documentation"
	cd $(RUST) && cargo run -- add "Deploy to production"
	cd $(RUST) && cargo run -- list
	cd $(RUST) && cargo run -- complete 1
	cd $(RUST) && cargo run -- list --pending

# Duplicate crates
.PHONY: dup
dup:
	rm -rf dup
	mkdir dup
	cp -a $(TCSLOG_CRATES) dup
