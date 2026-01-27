#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "🔨 Building preview-viewer worker..."

# Build frontend first
echo "📦 Building frontend..."
cd frontend
trunk build --release
cd ..

# Frontend outputs directly to ../dist (see frontend/trunk.toml)
# So dist is already created by trunk build
echo "📁 Frontend assets built to dist/"

# Build worker
echo "🦀 Building worker..."
worker-build --release

echo "✅ Build complete!"
