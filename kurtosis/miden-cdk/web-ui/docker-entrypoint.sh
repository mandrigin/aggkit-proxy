#!/bin/sh
# Inject runtime configuration into the served HTML.
# This runs before nginx starts (placed in /docker-entrypoint.d/).

BRIDGE_ADDRESS="${BRIDGE_ADDRESS:-0xc8cbebf950b9df44d987c8619f092bea980ff038}"
L1_CHAIN_ID="${L1_CHAIN_ID:-271828}"
HTML_DIR="/usr/share/nginx/html"

# Inject a tiny script block that sets window config vars
# before app.js loads. We prepend it to index.html's <head>.
sed -i "s|</head>|<script>window.__MIDEN_BRIDGE_ADDRESS=\"${BRIDGE_ADDRESS}\";window.__MIDEN_L1_CHAIN_ID=${L1_CHAIN_ID};</script></head>|" \
  "${HTML_DIR}/index.html"
