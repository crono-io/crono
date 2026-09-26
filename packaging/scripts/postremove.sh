#!/bin/sh
# Forget removed unit files. The `crono` account and edited configuration are
# kept, as distributions expect; purge or delete them by hand.
set -e

if [ -d /run/systemd/system ]; then
    systemctl daemon-reload >/dev/null 2>&1 || true
fi
