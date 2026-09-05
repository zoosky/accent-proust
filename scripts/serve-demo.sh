#!/usr/bin/env bash
# Build the WebAssembly package and serve the demo.
#
# Usage:
#   ./scripts/serve-demo.sh          # build, then serve on 8000
#   ./scripts/serve-demo.sh 9000     # a different port
#
# A static server is required rather than opening the file directly: the demo
# is an ES module and `init()` fetches the `.wasm`, and a browser refuses both
# over `file:`.

set -euo pipefail

cd "$(dirname "$0")/.."

PORT=${1:-8000}

echo "==> building the package"
OUT=demo/pkg ./scripts/build-npm.sh

echo
# `--bind 127.0.0.1` is IPv4 only, and a browser resolves `localhost` to
# `::1` first, so printing "localhost" here sends the reader to a refused
# connection. Print the address actually bound.
echo "==> http://127.0.0.1:$PORT/demo/"
echo "    Ctrl-C to stop."
echo
exec python3 -m http.server "$PORT" --bind 127.0.0.1
