#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "Building viewer worker..."

# Build frontend first
echo "Building frontend..."
cd ../frontend
trunk build --release --dist ../worker/dist
cd ../worker

echo "Frontend assets built to dist/"

# Determine the correct worker-build version from the resolved worker crate
WORKER_VERSION=$(cargo metadata --format-version 1 2>/dev/null | jq -r '.packages[] | select(.name == "worker") | .version')

if [ -z "$WORKER_VERSION" ]; then
    echo "Error: Could not determine worker version from cargo metadata"
    exit 1
fi

WORKER_MAJOR_MINOR=$(echo "$WORKER_VERSION" | cut -d. -f1,2)

case "$WORKER_MAJOR_MINOR" in
    "0.6")
        WORKER_BUILD_VERSION="0.1.11"
        ;;
    "0.7"|"0.8"|"0.9"|"1.0")
        WORKER_BUILD_VERSION="$WORKER_VERSION"
        ;;
    *)
        echo "Error: Unknown worker version $WORKER_VERSION - please update build.sh"
        exit 1
        ;;
esac

echo "Worker version: $WORKER_VERSION -> worker-build version: $WORKER_BUILD_VERSION"
cargo install -q worker-build --version "$WORKER_BUILD_VERSION" --locked

# Build worker
echo "Building worker..."
worker-build --release

echo "Build complete!"
