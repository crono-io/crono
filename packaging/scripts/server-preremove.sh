#!/bin/sh
# Stop and disable the server when the package is removed, not upgraded.
# dpkg passes `remove`; rpm passes 0 for an erase and 1 for an upgrade.
set -e

case "$1" in
    remove | 0)
        if [ -d /run/systemd/system ]; then
            systemctl --no-reload disable --now crono-server.service >/dev/null 2>&1 || true
        fi
        ;;
esac
