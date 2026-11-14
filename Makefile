APP_NAME    := CheCheViewer
BIN_NAME    := checheviewer
BUNDLE_DIR  := target/$(APP_NAME).app

# Ruta opcional a abrir: make run ARGS=~/Pictures
ARGS ?=

.PHONY: run
run:
	cargo run -- $(ARGS)

.PHONY: build
build:
	cargo build

.PHONY: release
release:
	cargo build --release

.PHONY: test
test:
	cargo test

.PHONY: fmt
fmt:
	cargo fmt

.PHONY: clippy
clippy:
	cargo clippy --all-targets -- -D warnings

.PHONY: check
check: fmt clippy test

.PHONY: clean
clean:
	cargo clean

# Arma target/CheCheViewer.app a partir del binario release.
.PHONY: bundle
bundle: release
	rm -rf "$(BUNDLE_DIR)"
	mkdir -p "$(BUNDLE_DIR)/Contents/MacOS" "$(BUNDLE_DIR)/Contents/Resources"
	cp bundle/Info.plist "$(BUNDLE_DIR)/Contents/Info.plist"
	cp target/release/$(BIN_NAME) "$(BUNDLE_DIR)/Contents/MacOS/$(BIN_NAME)"
	@[ -f bundle/AppIcon.icns ] && cp bundle/AppIcon.icns "$(BUNDLE_DIR)/Contents/Resources/" || true
	@echo "Bundle listo: $(BUNDLE_DIR)"

# Copia el bundle a /Applications y refresca Launch Services para que
# aparezca en "Abrir con".
.PHONY: install
install: bundle
	rm -rf "/Applications/$(APP_NAME).app"
	cp -R "$(BUNDLE_DIR)" /Applications/
	/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister \
		-f "/Applications/$(APP_NAME).app"
	@echo "Instalado en /Applications/$(APP_NAME).app"
