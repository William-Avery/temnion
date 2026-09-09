#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# Builds and packages Temnion release distributions for Linux and macOS.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

VERSION="${1:-}"
TARGET="${2:-}"
OUTPUT_DIR="${3:-${REPO_ROOT}/dist}"
SKIP_BUILD="${SKIP_BUILD:-false}"

# Detect version from Cargo.toml if not passed
if [[ -z "${VERSION}" ]]; then
    VERSION=$(grep -m1 '^version = ' "${REPO_ROOT}/Cargo.toml" | cut -d'"' -f2)
fi

# Detect host target if not passed
if [[ -z "${TARGET}" ]]; then
    TARGET=$(rustc -Vv | grep 'host:' | cut -d' ' -f2)
fi

echo -e "\033[1;36mPackaging Temnion v${VERSION} for ${TARGET}...\033[0m"

# 1. Build release binaries
if [[ "${SKIP_BUILD}" != "true" ]]; then
    echo -e "\033[1;33mBuilding release binaries (tem, temniond, tzeentch)...\033[0m"
    if [[ -n "${TARGET}" ]]; then
        cargo build --release --locked --bin tem --bin temniond --bin tzeentch --target "${TARGET}"
    else
        cargo build --release --locked --bin tem --bin temniond --bin tzeentch
    fi
fi

# 2. Locate built binaries
if [[ -n "${TARGET}" ]]; then
    TARGET_DIR="${REPO_ROOT}/target/${TARGET}/release"
else
    TARGET_DIR="${REPO_ROOT}/target/release"
fi

TEM_BIN="${TARGET_DIR}/tem"
TEMNIOND_BIN="${TARGET_DIR}/temniond"
TZEENTCH_BIN="${TARGET_DIR}/tzeentch"

if [[ ! -f "${TEM_BIN}" ]]; then
    echo "Error: missing binary ${TEM_BIN}" >&2
    exit 1
fi
if [[ ! -f "${TEMNIOND_BIN}" ]]; then
    echo "Error: missing binary ${TEMNIOND_BIN}" >&2
    exit 1
fi
if [[ ! -f "${TZEENTCH_BIN}" ]]; then
    echo "Error: missing binary ${TZEENTCH_BIN}" >&2
    exit 1
fi

# 3. Create stage directory
PACKAGE_NAME="temnion-v${VERSION}-${TARGET}"
STAGE_DIR="${OUTPUT_DIR}/${PACKAGE_NAME}"
rm -rf "${STAGE_DIR}"
mkdir -p "${STAGE_DIR}/bin"
mkdir -p "${STAGE_DIR}/config"
mkdir -p "${STAGE_DIR}/installer"
mkdir -p "${STAGE_DIR}/services/systemd"
mkdir -p "${STAGE_DIR}/services/windows"
mkdir -p "${STAGE_DIR}/studio-web"

# Copy binaries
install -m 755 "${TEM_BIN}" "${STAGE_DIR}/bin/"
install -m 755 "${TEMNIOND_BIN}" "${STAGE_DIR}/bin/"
install -m 755 "${TZEENTCH_BIN}" "${STAGE_DIR}/bin/"

# Copy Studio Desktop binary if built
STUDIO_BIN="${REPO_ROOT}/apps/temnion-studio/src-tauri/target/release/temnion-studio"
if [[ -f "${STUDIO_BIN}" ]]; then
    install -m 755 "${STUDIO_BIN}" "${STAGE_DIR}/bin/"
fi

# Copy Studio Web assets if built
if [[ -d "${REPO_ROOT}/apps/temnion-studio/dist" ]]; then
    cp -r "${REPO_ROOT}/apps/temnion-studio/dist/"* "${STAGE_DIR}/studio-web/"
fi

# Copy installer scripts
if [[ -d "${REPO_ROOT}/installer" ]]; then
    cp -r "${REPO_ROOT}/installer/"* "${STAGE_DIR}/installer/"
    chmod +x "${STAGE_DIR}/installer/"*.sh 2>/dev/null || true
fi

# Copy configs and services
cp "${REPO_ROOT}/temnion.example.toml" "${STAGE_DIR}/config/"
cp "${REPO_ROOT}/services/systemd/temniond.service" "${STAGE_DIR}/services/systemd/"
cp "${REPO_ROOT}/services/windows/install-service.ps1" "${STAGE_DIR}/services/windows/"
cp "${REPO_ROOT}/services/windows/uninstall-service.ps1" "${STAGE_DIR}/services/windows/"

# Copy documentation and licenses
cp "${REPO_ROOT}/README.md" "${STAGE_DIR}/"
cp "${REPO_ROOT}/CHANGELOG.md" "${STAGE_DIR}/"
cp "${REPO_ROOT}/LICENSE" "${STAGE_DIR}/" 2>/dev/null || true

# 4. Create tar.gz archive
mkdir -p "${OUTPUT_DIR}"
ARCHIVE_PATH="${OUTPUT_DIR}/${PACKAGE_NAME}.tar.gz"
rm -f "${ARCHIVE_PATH}"

echo -e "\033[1;33mCreating archive: ${ARCHIVE_PATH}...\033[0m"
tar -czf "${ARCHIVE_PATH}" -C "${OUTPUT_DIR}" "${PACKAGE_NAME}"

# 5. Compute SHA256 checksum
CHECKSUM_FILE="${ARCHIVE_PATH}.sha256"
if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "${ARCHIVE_PATH}" | awk '{print $1 "  " "'"${PACKAGE_NAME}.tar.gz"'"}' > "${CHECKSUM_FILE}"
elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "${ARCHIVE_PATH}" | awk '{print $1 "  " "'"${PACKAGE_NAME}.tar.gz"'"}' > "${CHECKSUM_FILE}"
fi

echo -e "\033[1;32mPackage successfully created!\033[0m"
echo "  Archive:  ${ARCHIVE_PATH}"
if [[ -f "${CHECKSUM_FILE}" ]]; then
    echo "  Checksum: $(cat "${CHECKSUM_FILE}")"
fi
