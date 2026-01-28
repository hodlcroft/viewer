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

# Build worker
echo "Building worker..."
worker-build --release

echo "Build complete!"
