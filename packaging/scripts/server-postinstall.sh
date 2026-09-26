#!/bin/sh
# Secure the configuration, reload systemd, and restart a running server after
# an upgrade. A first install never enables or starts the service: configure
# /etc/crono/server.env, then `systemctl enable --now crono-server.service`.
#
# Arguments follow the package manager: dpkg passes `configure [old-version]`,
# rpm passes the number of installed instances (1 install, 2 or more upgrade).
set -e

if [ -f /etc/crono/server.env ]; then
    chgrp crono /etc/crono/server.env
    chmod 0640 /etc/crono/server.env
fi

if [ -d /run/systemd/system ]; then
    systemctl daemon-reload >/dev/null 2>&1 || true
    case "$1" in
        configure) upgrade="${2:-}" ;;
        [0-9]*) if [ "$1" -ge 2 ]; then upgrade=yes; else upgrade=""; fi ;;
        *) upgrade="" ;;
    esac
    if [ -n "$upgrade" ]; then
        systemctl try-restart crono-server.service >/dev/null 2>&1 || true
    fi
fi
