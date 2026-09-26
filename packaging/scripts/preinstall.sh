#!/bin/sh
# Create the unprivileged `crono` system account before files are unpacked so
# packaged files and services can use it. Runs on every install and upgrade of
# crono-server and crono-worker and changes nothing when the account exists.
set -e

if ! getent group crono >/dev/null 2>&1; then
    groupadd --system crono
fi
if ! getent passwd crono >/dev/null 2>&1; then
    nologin="$(command -v nologin 2>/dev/null || echo /usr/sbin/nologin)"
    useradd --system --gid crono --home-dir /var/lib/crono --no-create-home \
        --shell "$nologin" --comment "Crono service account" crono
fi
