#!/usr/bin/env bash
# ==============================================================================
# check-setup.sh - Verify project environment, IntelliJ IDEA, gitignore & lint/format
# ==============================================================================
# Usage:
#   ./scripts/check-setup.sh         # Run diagnostics
#   ./scripts/check-setup.sh --fix   # Run diagnostics and auto-repair configs
# ==============================================================================

set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

AUTO_FIX=false
if [[ "${1:-}" == "--fix" ]]; then
  AUTO_FIX=true
fi

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
BOLD='\033[1m'
NC='\033[0m' # No Color

PASSED_CHECKS=0
WARNING_CHECKS=0
FAILED_CHECKS=0

pass() {
  echo -e "  ${GREEN}[✓ PASS]${NC} $1"
  PASSED_CHECKS=$((PASSED_CHECKS + 1))
}

warn() {
  echo -e "  ${YELLOW}[! WARN]${NC} $1"
  WARNING_CHECKS=$((WARNING_CHECKS + 1))
}

fail() {
  echo -e "  ${RED}[✗ FAIL]${NC} $1"
  FAILED_CHECKS=$((FAILED_CHECKS + 1))
}

info() {
  echo -e "  ${BLUE}[INFO]${NC} $1"
}

section() {
  echo -e "\n${BOLD}$1${NC}"
  echo -e "${BOLD}$(printf '%.0s-' {1..50})${NC}"
}

# ------------------------------------------------------------------------------
# 1. Toolchain & Runtime Prerequisites
# ------------------------------------------------------------------------------
section "1. Toolchain & Runtimes"

# Node.js
if command -v node >/dev/null 2>&1; then
  NODE_VER=$(node -v)
  NODE_MAJOR=$(echo "$NODE_VER" | sed -E 's/^v([0-9]+).*/\1/')
  if [[ "$NODE_MAJOR" -ge 22 ]]; then
    pass "Node.js $NODE_VER (meets requirement >= 22)"
  else
    warn "Node.js $NODE_VER detected, but package.json requires >= 22"
  fi
else
  fail "Node.js is not installed or not in PATH"
fi

# pnpm
if command -v pnpm >/dev/null 2>&1; then
  PNPM_VER=$(pnpm -v)
  pass "pnpm $PNPM_VER installed"
else
  fail "pnpm is not installed. Install via 'npm i -g pnpm' or 'corepack enable'"
fi

# Rust / Cargo
if command -v rustc >/dev/null 2>&1; then
  RUST_VER=$(rustc --version)
  pass "$RUST_VER"
else
  fail "rustc is not installed. Install via rustup (https://rustup.rs)"
fi

if command -v cargo >/dev/null 2>&1; then
  CARGO_VER=$(cargo --version)
  pass "$CARGO_VER"
else
  fail "cargo is not installed or not in PATH"
fi

# Linux Tauri System Dependencies (if on Linux)
if [[ "$(uname -s)" == "Linux" ]]; then
  if command -v pkg-config >/dev/null 2>&1; then
    pass "pkg-config installed"
    if pkg-config --exists webkit2gtk-4.1; then
      WEBKIT_VER=$(pkg-config --modversion webkit2gtk-4.1)
      pass "webkit2gtk-4.1 ($WEBKIT_VER) detected"
    elif pkg-config --exists webkit2gtk-4.0; then
      WEBKIT_VER=$(pkg-config --modversion webkit2gtk-4.0)
      pass "webkit2gtk-4.0 ($WEBKIT_VER) detected"
    else
      warn "Neither webkit2gtk-4.1 nor webkit2gtk-4.0 found via pkg-config (needed for Tauri dev/build)"
    fi
  else
    warn "pkg-config not found. Tauri build may require pkg-config on Linux"
  fi

  # GStreamer multimedia plugins check
  if command -v gst-inspect-1.0 >/dev/null 2>&1; then
    if gst-inspect-1.0 autoaudiosink >/dev/null 2>&1; then
      pass "GStreamer autoaudiosink available"
    else
      warn "GStreamer 'autoaudiosink' missing (WebKitWebProcess will log GLib warnings). Install 'gst-plugins-good' (sudo pacman -S gst-plugins-good)"
    fi
  fi
fi

# ------------------------------------------------------------------------------
# 2. Project Dependencies & Files
# ------------------------------------------------------------------------------
section "2. Project Files & Dependencies"

# package.json
if [[ -f "package.json" ]]; then
  pass "package.json found"
else
  fail "package.json missing from project root"
fi

# node_modules
if [[ -d "node_modules" ]]; then
  if [[ -f "node_modules/@tauri-apps/cli/package.json" ]]; then
    pass "node_modules present and dependencies installed"
  else
    warn "node_modules directory found, but packages might be incomplete. Run 'pnpm install'"
  fi
else
  fail "node_modules directory missing. Run 'pnpm install'"
fi

# Cargo workspace
if [[ -f "src-tauri/Cargo.toml" ]]; then
  pass "src-tauri/Cargo.toml found"
else
  fail "src-tauri/Cargo.toml missing"
fi

# Environment
if [[ -f ".env" ]]; then
  pass ".env file exists"
elif [[ -f ".env.example" ]]; then
  warn ".env file not found, but .env.example exists"
fi

# ------------------------------------------------------------------------------
# 3. IntelliJ IDEA Configuration & Project Structure
# ------------------------------------------------------------------------------
section "3. IntelliJ IDEA Configuration & Project Structure"

if [[ ! -d ".idea" ]]; then
  warn ".idea directory does not exist. Open this project in IntelliJ to generate it"
else
  pass ".idea directory present"

  # 3.1 Project Structure Settings (misc.xml)
  MISC_XML=".idea/misc.xml"
  if [[ -f "$MISC_XML" ]]; then
    pass "Project Structure settings found ($MISC_XML)"

    # Compiler Output
    COMPILER_OUT_URL=$(grep -oE 'output url="[^"]+"' "$MISC_XML" | head -n1 | sed -e 's/output url="//' -e 's/"//' || echo "")
    if [[ -n "$COMPILER_OUT_URL" ]]; then
      COMPILER_OUT_DIR=$(echo "$COMPILER_OUT_URL" | sed -e 's|file://||' -e 's|[$]PROJECT_DIR[$]/||' -e 's|^/||')
      pass "Compiler output configured: $COMPILER_OUT_URL (directory: '$COMPILER_OUT_DIR')"

      # Verify that this compiler output directory is covered by .gitignore
      if [[ -f ".gitignore" ]] && grep -qE "(^|/)$COMPILER_OUT_DIR(/|$)" .gitignore; then
        pass "Compiler output ('$COMPILER_OUT_DIR') is covered in .gitignore"
      else
        warn "Compiler output directory ('$COMPILER_OUT_DIR') is NOT in .gitignore! Build artifacts may pollute git"
        if [[ "$AUTO_FIX" == true ]]; then
          echo "$COMPILER_OUT_DIR/" >> .gitignore
          info "Auto-repaired: Added '$COMPILER_OUT_DIR/' to .gitignore"
        fi
      fi
    else
      warn "No compiler output URL configured in $MISC_XML"
    fi

    # Project SDK / Language Level
    if grep -q 'project-jdk-name=' "$MISC_XML"; then
      PROJECT_SDK=$(grep -oE 'project-jdk-name="[^"]+"' "$MISC_XML" | head -n1 | sed -e 's/project-jdk-name="//' -e 's/"//')
      pass "Project SDK configured: $PROJECT_SDK"
    else
      pass "Project SDK: <No SDK> (Standard for Tauri/Vue/Rust project; JVM SDK optional)"
    fi
  else
    warn "$MISC_XML not found"
  fi

  # 3.2 modules.xml & Active .iml
  MODULES_XML=".idea/modules.xml"
  if [[ -f "$MODULES_XML" ]]; then
    pass "$MODULES_XML found"

    # Extract module file path ($PROJECT_DIR$ macro replacement)
    IML_REL_PATH=$(grep -oE 'filepath="[^"]+"' "$MODULES_XML" | head -n1 | sed -e 's/filepath="//' -e 's/"//' -e 's|[$]PROJECT_DIR[$]/||')

    if [[ -z "$IML_REL_PATH" ]]; then
      fail "Could not parse module filepath from $MODULES_XML"
    else
      ACTIVE_IML="$PROJECT_ROOT/$IML_REL_PATH"
      if [[ -f "$ACTIVE_IML" ]]; then
        pass "Active module file: $IML_REL_PATH"

        # Check module type
        if grep -q 'type="WEB_MODULE"' "$ACTIVE_IML"; then
          pass "Module type is WEB_MODULE (correct for Vue/TypeScript/Vite)"
        elif grep -q 'type="JAVA_MODULE"' "$ACTIVE_IML"; then
          fail "Module type is JAVA_MODULE! This project has no Java code and requires WEB_MODULE"
          if [[ "$AUTO_FIX" == true ]]; then
            sed -i 's/type="JAVA_MODULE"/type="WEB_MODULE"/' "$ACTIVE_IML"
            info "Auto-repaired: Set module type to WEB_MODULE in $ACTIVE_IML"
          fi
        else
          warn "Module type in $ACTIVE_IML is $(grep -oE 'type="[^"]+"' "$ACTIVE_IML" || echo "unknown")"
        fi

        # Check content root
        # Dangerous: url="file://$MODULE_DIR$/.." sets content root to parent Projects folder
        if grep -q 'url="file://\$MODULE_DIR\$/\.\."' "$ACTIVE_IML"; then
          fail "Content root points to '\$MODULE_DIR$/..' (evaluates to parent directory /home/paul/Projects!)"
          if [[ "$AUTO_FIX" == true ]]; then
            sed -i 's|url="file://\$MODULE_DIR\$/\.\."|url="file://\$MODULE_DIR\$"|' "$ACTIVE_IML"
            info "Auto-repaired: Corrected content root to '\$MODULE_DIR$' in $ACTIVE_IML"
          fi
        elif grep -q 'url="file://\$MODULE_DIR\$"' "$ACTIVE_IML"; then
          pass "Content root correctly mapped to project directory (\$MODULE_DIR$)"
        else
          warn "Unusual content root configuration in $ACTIVE_IML"
        fi

        # Check essential exclusions
        MISSING_EXCLUDES=()
        for folder in "dist" "node_modules" "src-tauri/target"; do
          if ! grep -q "url=\"file://\$MODULE_DIR\$/$folder\"" "$ACTIVE_IML" && ! grep -q "url=\"file://\$MODULE_DIR\$/\.\./$folder\"" "$ACTIVE_IML"; then
            MISSING_EXCLUDES+=("$folder")
          fi
        done

        if [[ ${#MISSING_EXCLUDES[@]} -eq 0 ]]; then
          pass "Heavy build folders excluded from indexing: dist, node_modules, src-tauri/target"
        else
          warn "Missing exclusions in $ACTIVE_IML: ${MISSING_EXCLUDES[*]}"
          if [[ "$AUTO_FIX" == true ]]; then
            cat << 'EOF' > "$ACTIVE_IML"
<?xml version="1.0" encoding="UTF-8"?>
<module type="WEB_MODULE" version="4">
  <component name="NewModuleRootManager" inherit-compiler-output="true">
    <exclude-output />
    <content url="file://$MODULE_DIR$">
      <excludeFolder url="file://$MODULE_DIR$/dist" />
      <excludeFolder url="file://$MODULE_DIR$/node_modules" />
      <excludeFolder url="file://$MODULE_DIR$/src-tauri/target" />
    </content>
    <orderEntry type="sourceFolder" forTests="false" />
  </component>
</module>
EOF
            info "Auto-repaired: Re-wrote $ACTIVE_IML with standard WEB_MODULE and excluded folders"
          fi
        fi

      else
        fail "Referenced module file does not exist: $ACTIVE_IML"
      fi
    fi
  else
    fail "$MODULES_XML missing"
  fi

  # VCS mapping
  if [[ -f ".idea/vcs.xml" ]] && grep -q 'vcs="Git"' ".idea/vcs.xml"; then
    pass "Git VCS mapping configured in .idea/vcs.xml"
  else
    warn "Git VCS mapping missing or incomplete in .idea/vcs.xml"
  fi

  # Native Run configurations (npm and Cargo)
  RUN_CONFIGS_DIR=".idea/runConfigurations"
  if [[ -d "$RUN_CONFIGS_DIR" ]]; then
    EXPECTED_CONFIGS=(
      "Check_Setup.xml" "Format.xml" "Lint.xml" "Tauri_Dev.xml"
      "Frontend_Dev.xml" "Frontend_Build.xml" "Cargo_Check.xml"
      "Cargo_Test.xml" "Cargo_Clippy.xml" "Build_Debug.xml"
      "Build_Production.xml" "Typecheck.xml"
    )
    FOUND_COUNT=0
    NATIVE_COUNT=0
    for cfg in "${EXPECTED_CONFIGS[@]}"; do
      if [[ -f "$RUN_CONFIGS_DIR/$cfg" ]]; then
        FOUND_COUNT=$((FOUND_COUNT + 1))
        if grep -qE 'type="(js\.build_tools\.npm|CargoCommandRunConfiguration)"' "$RUN_CONFIGS_DIR/$cfg"; then
          NATIVE_COUNT=$((NATIVE_COUNT + 1))
        fi
      fi
    done
    if [[ $FOUND_COUNT -eq ${#EXPECTED_CONFIGS[@]} ]]; then
      pass "All standard IntelliJ run configurations found ($FOUND_COUNT/${#EXPECTED_CONFIGS[@]})"
    else
      warn "Some run configurations missing in $RUN_CONFIGS_DIR ($FOUND_COUNT/${#EXPECTED_CONFIGS[@]} present)"
    fi
    if [[ $NATIVE_COUNT -eq $FOUND_COUNT ]]; then
      pass "All IntelliJ run configurations use native IDE runners (npm / Cargo)"
    else
      warn "$((FOUND_COUNT - NATIVE_COUNT)) run configurations are still shell scripts instead of native runners"
    fi
  else
    warn ".idea/runConfigurations directory missing"
  fi

  # Prettier integration in IntelliJ
  if [[ -f ".idea/prettier.xml" ]]; then
    pass "Prettier configuration found (.idea/prettier.xml)"
  else
    warn "Prettier configuration not found in .idea/prettier.xml"
  fi
fi

# ------------------------------------------------------------------------------
# 4. Git Ignore & Repository Hygiene
# ------------------------------------------------------------------------------
section "4. Git Ignore & Repository Hygiene"

if [[ ! -f ".gitignore" ]]; then
  fail ".gitignore missing from project root"
else
  pass ".gitignore present"

  # 4.1 Anti-pattern: Hardcoded absolute paths
  ABSOLUTE_PATHS=$(grep -nE '^[^#]*(/home/|/Users/|[a-zA-Z]:[/\\\\])' .gitignore || true)
  if [[ -n "$ABSOLUTE_PATHS" ]]; then
    fail "Hardcoded absolute path found in .gitignore (violates portable best practice):"
    while IFS= read -r line; do
      echo -e "       ${RED}Line $line${NC}"
    done <<< "$ABSOLUTE_PATHS"
    if [[ "$AUTO_FIX" == true ]]; then
      sed -i -E 's|/[^#]*/\.serena\*|.serena*|g' .gitignore
      info "Auto-repaired: Replaced hardcoded path with '.serena*' in .gitignore"
    fi
  else
    pass "No hardcoded absolute paths in .gitignore"
  fi

  # 4.2 Standard practice patterns coverage
  CHECK_PATTERNS=(
    "node_modules:Node dependencies"
    "dist:Frontend build outputs"
    "\.env:Environment secret files"
    "\.idea:IntelliJ IDE configuration"
    "\.vscode:VS Code configuration"
    "\.DS_Store:macOS system files"
  )

  for entry in "${CHECK_PATTERNS[@]}"; do
    pat="${entry%%:*}"
    desc="${entry#*:}"
    if grep -qE "(^|/)$pat(/|$)" .gitignore; then
      pass "Covers $desc ($pat)"
    else
      warn "Missing standard ignore pattern for $desc ($pat)"
    fi
  done

  # IntelliJ compiler output (from Project Structure, e.g. out/)
  if grep -qE '(^|/)out(/|$)' .gitignore; then
    pass "Covers IntelliJ compiler output (out/)"
  else
    warn "Missing standard ignore pattern for IntelliJ compiler output (out/)"
    if [[ "$AUTO_FIX" == true ]]; then
      echo "out/" >> .gitignore
      info "Auto-repaired: Added out/ to .gitignore"
    fi
  fi

  # Rust target check (root or src-tauri/.gitignore)
  if grep -qE '(^|/)target(/|$)' .gitignore || ( [[ -f "src-tauri/.gitignore" ]] && grep -qE '(^|/)target(/|$)' "src-tauri/.gitignore" ); then
    pass "Covers Rust build artifacts (target/)"
  else
    warn "Missing standard ignore pattern for Rust build artifacts (target/)"
  fi

  # Windows OS files (Thumbs.db)
  if grep -qE 'Thumbs\.db' .gitignore; then
    pass "Covers Windows system files (Thumbs.db)"
  else
    warn "Missing standard ignore pattern for Windows system files (Thumbs.db)"
    if [[ "$AUTO_FIX" == true ]]; then
      echo "Thumbs.db" >> .gitignore
      echo "desktop.ini" >> .gitignore
      info "Auto-repaired: Added Thumbs.db and desktop.ini to .gitignore"
    fi
  fi

  # Tooling cache ignores
  if grep -qE '\.eslintcache' .gitignore; then
    pass "Covers ESLint cache (.eslintcache)"
  else
    warn "Missing ignore for ESLint cache (.eslintcache)"
    if [[ "$AUTO_FIX" == true ]]; then
      echo ".eslintcache" >> .gitignore
      info "Auto-repaired: Added .eslintcache to .gitignore"
    fi
  fi

  # 4.3 Secret leaks check (ensure secrets are NOT tracked in git)
  TRACKED_SECRETS=$(git ls-files .env '*.pem' '*.key' '*.pfx' id_rsa 2>/dev/null || true)
  if [[ -n "$TRACKED_SECRETS" ]]; then
    fail "Sensitive files tracked by Git: $TRACKED_SECRETS"
  else
    pass "No sensitive secrets (.env, private keys) tracked by Git"
  fi

  # 4.4 Tracked files conflicting with .gitignore rules
  CONFLICTING_FILES=$(git ls-files -c -i --exclude-standard 2>/dev/null || true)
  if [[ -n "$CONFLICTING_FILES" ]]; then
    COUNT=$(echo "$CONFLICTING_FILES" | wc -l)
    warn "Tracked files ($COUNT) currently match .gitignore patterns (may cause unexpected git behavior):"
    while IFS= read -r f; do
      echo -e "       ${YELLOW}→ $f${NC}"
    done <<< "$CONFLICTING_FILES"
    echo -e "       ${BLUE}Hint: If these files should be tracked (e.g. documentation screenshots), refine the .gitignore rule.${NC}"
  else
    pass "No conflicts between tracked files and .gitignore patterns"
  fi

  # 4.5 Duplicate rules check
  DUPLICATES=$(grep -vE '^\s*(#|$)' .gitignore | sort | uniq -d || true)
  if [[ -n "$DUPLICATES" ]]; then
    warn "Duplicate rules found in .gitignore: $(echo $DUPLICATES | tr '\n' ' ')"
  else
    pass "No duplicate rules in .gitignore"
  fi
fi

# ------------------------------------------------------------------------------
# 5. Lint & Format Configuration
# ------------------------------------------------------------------------------
section "5. Lint & Format Configuration"

# ESLint Configuration
if [[ -f "eslint.config.js" || -f "eslint.config.mjs" ]]; then
  pass "ESLint flat configuration found (eslint.config.js)"
  # Check package.json scripts
  if grep -q '"lint":' package.json; then
    pass "npm script 'lint' defined in package.json"
  else
    warn "npm script 'lint' missing from package.json"
  fi
  if grep -q '"lint:fix":' package.json; then
    pass "npm script 'lint:fix' defined in package.json"
  else
    warn "npm script 'lint:fix' missing from package.json"
  fi
else
  warn "ESLint configuration file not found"
fi

# Prettier Configuration
if [[ -f ".prettierrc" || -f ".prettierrc.json" || -f ".prettierrc.js" || -f "prettier.config.js" ]]; then
  pass "Prettier configuration file found (.prettierrc*)"
else
  warn "Prettier configuration file missing (.prettierrc.json)"
fi

if [[ -f ".prettierignore" ]]; then
  pass "Prettier ignore file found (.prettierignore)"
else
  warn ".prettierignore missing"
fi

if grep -q '"format":' package.json; then
  pass "npm script 'format' defined in package.json"
else
  warn "npm script 'format' missing from package.json"
fi

# EditorConfig
if [[ -f ".editorconfig" ]]; then
  pass "EditorConfig found (.editorconfig) for cross-IDE consistency"
else
  warn ".editorconfig missing"
fi

# Rust Format & Lint (src-tauri)
if [[ -d "src-tauri" ]]; then
  if command -v cargo >/dev/null 2>&1; then
    pass "Rust cargo fmt and cargo clippy toolchain available"
  fi
fi

# ------------------------------------------------------------------------------
# Summary & Status
# ------------------------------------------------------------------------------
section "Setup Verification Summary"
echo -e "  Passed:   ${GREEN}$PASSED_CHECKS${NC}"
echo -e "  Warnings: ${YELLOW}$WARNING_CHECKS${NC}"
echo -e "  Failed:   ${RED}$FAILED_CHECKS${NC}"

if [[ $FAILED_CHECKS -eq 0 ]]; then
  echo -e "\n${GREEN}${BOLD}✓ Project and IntelliJ IDE are correctly configured!${NC}\n"
  exit 0
else
  echo -e "\n${RED}${BOLD}✗ Setup issues detected.${NC}"
  if [[ "$AUTO_FIX" == false ]]; then
    echo -e "  Tip: Run ${BOLD}./scripts/check-setup.sh --fix${NC} to automatically repair configuration issues.\n"
  fi
  exit 1
fi
