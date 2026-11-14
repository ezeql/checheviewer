APP_NAME    := CheCheViewer
BIN_NAME    := checheviewer
BUNDLE_DIR  := target/$(APP_NAME).app
LSREGISTER  := /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister

# Optional path to open: make run ARGS=~/Pictures
ARGS ?=

.DEFAULT_GOAL := help

.PHONY: help
help: ## List the targets
	@grep -E '^[a-z]+:.*## ' $(MAKEFILE_LIST) | awk -F':.*## ' '{printf "  make %-10s %s\n", $$1, $$2}'

.PHONY: run
run: ## Run a debug build (ARGS=path to open)
	cargo run -- $(ARGS)

.PHONY: build
build: ## Debug build
	cargo build

.PHONY: release
release: ## Optimized build
	cargo build --release --locked

.PHONY: test
test: ## Run the tests
	cargo test

.PHONY: fmt
fmt: ## Format the code
	cargo fmt

.PHONY: clippy
clippy: ## Lint
	cargo clippy --all-targets -- -D warnings

.PHONY: check
check: fmt clippy test ## Format, lint and test

.PHONY: clean
clean: ## Remove build output
	cargo clean

.PHONY: bundle
bundle: release ## Build target/CheCheViewer.app
	rm -rf "$(BUNDLE_DIR)"
	mkdir -p "$(BUNDLE_DIR)/Contents/MacOS" "$(BUNDLE_DIR)/Contents/Resources"
	cp bundle/Info.plist "$(BUNDLE_DIR)/Contents/Info.plist"
	cp target/release/$(BIN_NAME) "$(BUNDLE_DIR)/Contents/MacOS/$(BIN_NAME)"
	@[ -f bundle/AppIcon.icns ] && cp bundle/AppIcon.icns "$(BUNDLE_DIR)/Contents/Resources/" || true
	@echo "Bundle ready: $(BUNDLE_DIR)"

# Registering with Launch Services is what makes it show up in Finder's
# "Open With" menu.
.PHONY: install
install: bundle ## Build and install into /Applications
	rm -rf "/Applications/$(APP_NAME).app"
	cp -R "$(BUNDLE_DIR)" /Applications/
	$(LSREGISTER) -f "/Applications/$(APP_NAME).app"
	@echo "Installed to /Applications/$(APP_NAME).app"

.PHONY: uninstall
uninstall: ## Remove it from /Applications
	-$(LSREGISTER) -u "/Applications/$(APP_NAME).app" 2>/dev/null
	rm -rf "/Applications/$(APP_NAME).app"
	@echo "Removed /Applications/$(APP_NAME).app"
