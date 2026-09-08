#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# Interactive PostgreSQL-style Component Installer & Setup Wizard for Temnion (Linux/macOS)

set -e

SILENT=0
DB_NAME="temnion_default"
PORT=9180
FLIGHT_PORT=9181
BIND_HOST="127.0.0.1"
USERNAME="temnion_admin"
PASSWORD=""
INSTALL_DIR="/usr/local/temnion"
DATA_DIR="/var/lib/temnion/data"

COMP_TEM=1
COMP_TEMNIOND=1
COMP_STUDIO=1
COMP_TZEENTCH=1
COMP_SERVICE=0

if [ "$(id -u)" -ne 0 ]; then
  INSTALL_DIR="$HOME/.local/temnion"
  DATA_DIR="$HOME/.local/share/temnion/data"
fi

print_banner() {
  echo -e "\033[1;36m================================================================================\033[0m"
  echo -e "\033[1;37m        TEMNION ENTERPRISE TEMPORAL DATABASE - SETUP WIZARD                    \033[0m"
  echo -e "\033[0;37m        Deterministic Bounded Architecture | Rust 2024 | forbid(unsafe_code)   \033[0m"
  echo -e "\033[1;36m================================================================================\033[0m"
  echo ""
}

# Parse flags
while [[ $# -gt 0 ]]; do
  case "$1" in
    --silent|-s)
      SILENT=1
      shift
      ;;
    --port)
      PORT="$2"
      shift 2
      ;;
    --db)
      DB_NAME="$2"
      shift 2
      ;;
    --user)
      USERNAME="$2"
      shift 2
      ;;
    --password)
      PASSWORD="$2"
      shift 2
      ;;
    --install-dir)
      INSTALL_DIR="$2"
      shift 2
      ;;
    --data-dir)
      DATA_DIR="$2"
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done

if [ "$SILENT" -eq 0 ]; then
  print_banner
  echo -e "\033[1;32mWelcome to the Temnion Database Setup Wizard.\033[0m"
  echo "This installer configures database credentials, network ports, and components."
  echo ""
  read -r -p "Database Name [$DB_NAME]: " in_db
  [ -n "$in_db" ] && DB_NAME="$in_db"

  read -r -p "TNP Server Port [$PORT]: " in_port
  [ -n "$in_port" ] && PORT="$in_port"

  read -r -p "Flight Port [$FLIGHT_PORT]: " in_fl
  [ -n "$in_fl" ] && FLIGHT_PORT="$in_fl"

  read -r -p "Superuser Username [$USERNAME]: " in_user
  [ -n "$in_user" ] && USERNAME="$in_user"

  if [ -z "$PASSWORD" ]; then
    read -r -s -p "Enter Password / Auth Token (leave blank for random): " in_pass
    echo ""
    if [ -z "$in_pass" ]; then
      PASSWORD=$(openssl rand -hex 16 2>/dev/null || head -c 16 /dev/urandom | xxd -p)
      echo -e "\033[1;32mGenerated secure token: $PASSWORD\033[0m"
    else
      PASSWORD="$in_pass"
    fi
  fi

  read -r -p "Installation Directory [$INSTALL_DIR]: " in_inst
  [ -n "$in_inst" ] && INSTALL_DIR="$in_inst"

  read -r -p "Database Store Directory [$DATA_DIR]: " in_data
  [ -n "$in_data" ] && DATA_DIR="$in_data"
fi

if [ -z "$PASSWORD" ]; then
  PASSWORD="temnion_secret_token"
fi

# Create target directories
BIN_DIR="$INSTALL_DIR/bin"
CONF_DIR="$INSTALL_DIR/config"
mkdir -p "$BIN_DIR" "$CONF_DIR" "$DATA_DIR"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Copy built binaries if available
for bin in tem temniond tzeentch; do
  if [ -f "$REPO_ROOT/target/release/$bin" ]; then
    cp "$REPO_ROOT/target/release/$bin" "$BIN_DIR/$bin"
    chmod +x "$BIN_DIR/$bin"
    echo -e "  \033[1;32m[✓]\033[0m Installed $bin -> $BIN_DIR/$bin"
  elif [ -f "$REPO_ROOT/target/debug/$bin" ]; then
    cp "$REPO_ROOT/target/debug/$bin" "$BIN_DIR/$bin"
    chmod +x "$BIN_DIR/$bin"
    echo -e "  \033[1;32m[✓]\033[0m Installed $bin -> $BIN_DIR/$bin"
  fi
done

# Write server temnion.toml
cat <<EOF > "$CONF_DIR/temnion.toml"
# Temnion Authoritative Server Configuration
[database]
database_name = "$DB_NAME"
admin_user = "$USERNAME"
auth_token = "$PASSWORD"

[storage]
data_dir = "$DATA_DIR"
source_id = 1
source_epoch = 1

[network]
server_id = "temniond-primary"
tnp_bind = "$BIND_HOST:$PORT"
flight_bind = "$BIND_HOST:$FLIGHT_PORT"
mcp_enabled = true

[maintenance]
maintenance_interval_secs = 60
EOF
echo -e "  \033[1;32m[✓]\033[0m Generated server configuration -> $CONF_DIR/temnion.toml"

# Write user connection profile
USER_CONF_DIR="$HOME/.temnion"
mkdir -p "$USER_CONF_DIR"
cat <<EOF > "$USER_CONF_DIR/connections.toml"
[default]
name = "Local Primary ($DB_NAME)"
host = "$BIND_HOST"
port = $PORT
flight_port = $FLIGHT_PORT
database = "$DB_NAME"
username = "$USERNAME"
auth_token = "$PASSWORD"
EOF
echo -e "  \033[1;32m[✓]\033[0m Configured client connection profile -> $USER_CONF_DIR/connections.toml"

print_banner
echo -e "\033[1;32m================================================================================\033[0m"
echo -e "\033[1;32m                    TEMNION INSTALLATION COMPLETED                              \033[0m"
echo -e "\033[1;32m================================================================================\033[0m"
echo ""
echo "Connection URI: temnion://$USERNAME:$PASSWORD@$BIND_HOST:$PORT/$DB_NAME"
echo ""
echo "Quickstart Commands:"
echo "  1. Test Connection: $BIN_DIR/tzeentch status"
echo "  2. Start Daemon:    $BIN_DIR/temniond run --config $CONF_DIR/temnion.toml"
echo "  3. Query:           $BIN_DIR/tem query 'FROM temnion SELECT * LIMIT 10'"
echo ""
EOF
