#!/bin/sh
# Prepare the worker's home, secure its configuration, and reload systemd. A
# first install never enables or starts the service: configure
# /etc/crono/worker.env, then `systemctl enable --now crono-worker.service`.
#
# Upgrades do not restart a running worker: stopping it waits for in-flight
# executions, which could block the package manager for a long time. Restart it
# when convenient to run the new version.
set -e

install -d -o crono -g crono -m 0750 /var/lib/crono
if [ -f /etc/crono/worker.env ]; then
    chgrp crono /etc/crono/worker.env
    chmod 0640 /etc/crono/worker.env
fi

if [ -d /run/systemd/system ]; then
    systemctl daemon-reload >/dev/null 2>&1 || true
    if systemctl is-active --quiet crono-worker.service 2>/dev/null; then
        echo "crono-worker is running the previous version; restart crono-worker.service to upgrade it."
    fi
fi
