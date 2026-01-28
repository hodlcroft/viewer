#!/bin/bash
set -euo pipefail

# Publish collection to Cloudflare R2
# Usage: ./scripts/publish.sh <slug>
#
# Example: ./scripts/publish.sh blackflag
#
# Uploads to R2:
#   collections/<slug>/collection.bin
#   collections/<slug>/sprites/0000.webp, 0001.webp, ...
#   collections/<slug>/hcf/images_000.hcf, images_001.hcf, ...
#
# Prerequisites:
#   - wrangler CLI installed and authenticated
#   - Collection synced with `viewer sync cardano <policy_id>`
#   - Config file with slug defined

# Number of parallel uploads
PARALLEL_JOBS="${PARALLEL_JOBS:-8}"

# Parse arguments
if [ -z "${1:-}" ]; then
    echo "Usage: $0 <slug>"
    echo ""
    echo "Arguments:"
    echo "  slug    Collection slug (e.g., blackflag)"
    echo ""
    echo "Environment variables:"
    echo "  PARALLEL_JOBS  Number of parallel uploads (default: 8)"
    echo "  R2_BUCKET      Target R2 bucket (default: hodlcroft)"
    echo ""
    echo "Example:"
    echo "  $0 blackflag"
    echo "  PARALLEL_JOBS=16 $0 blackflag"
    exit 1
fi

SLUG="$1"

# Configuration
R2_BUCKET="${R2_BUCKET:-hodlcroft}"

# Find the build directory by looking up the policy ID from config
CONFIG_FILE=$(grep -l "slug = \"$SLUG\"" configs/cardano/*.toml 2>/dev/null | head -1)
if [ -z "$CONFIG_FILE" ]; then
    echo "Error: No config file found with slug '$SLUG'"
    echo "Make sure you have a config file in configs/cardano/ with 'slug = \"$SLUG\"'"
    exit 1
fi

POLICY_ID=$(basename "$CONFIG_FILE" .toml)
BUILD_DIR=".build/$POLICY_ID"

echo "Publishing collection to R2"
echo "   Slug:      $SLUG"
echo "   Policy ID: $POLICY_ID"
echo "   R2 Bucket: $R2_BUCKET"
echo "   R2 Path:   collections/$SLUG/"
echo "   Parallel:  $PARALLEL_JOBS jobs"
echo ""

# Validate build directory exists
if [ ! -d "$BUILD_DIR" ]; then
    echo "Error: Build directory not found: $BUILD_DIR"
    echo "Run 'viewer sync cardano $POLICY_ID' first"
    exit 1
fi

# Validate required files exist
if [ ! -f "$BUILD_DIR/collection.bin" ]; then
    echo "Error: collection.bin not found in $BUILD_DIR"
    exit 1
fi

if [ ! -d "$BUILD_DIR/sprites" ]; then
    echo "Error: sprites directory not found in $BUILD_DIR"
    exit 1
fi

if [ ! -d "$BUILD_DIR/hcf" ]; then
    echo "Error: hcf directory not found in $BUILD_DIR"
    exit 1
fi

# Count files
SPRITE_COUNT=$(ls "$BUILD_DIR"/sprites/*.webp 2>/dev/null | wc -l | tr -d ' ')
HCF_COUNT=$(ls "$BUILD_DIR"/hcf/*.hcf 2>/dev/null | wc -l | tr -d ' ')

if [ "$SPRITE_COUNT" -eq 0 ]; then
    echo "Error: No sprite files found in $BUILD_DIR/sprites/"
    exit 1
fi

if [ "$HCF_COUNT" -eq 0 ]; then
    echo "Error: No HCF files found in $BUILD_DIR/hcf/"
    exit 1
fi

# Report sizes
echo "Bundle contents:"
SIZE=$(du -h "$BUILD_DIR/collection.bin" | cut -f1)
echo "   collection.bin: $SIZE"

SPRITE_TOTAL=$(du -ch "$BUILD_DIR"/sprites/*.webp | tail -1 | cut -f1)
echo "   sprites/: $SPRITE_COUNT files ($SPRITE_TOTAL)"

HCF_TOTAL=$(du -ch "$BUILD_DIR"/hcf/*.hcf | tail -1 | cut -f1)
echo "   hcf/: $HCF_COUNT files ($HCF_TOTAL)"
echo ""

# Upload to R2
R2_PREFIX="collections/$SLUG"

echo "Uploading to R2..."

# Upload collection.bin first (single file)
echo "   Uploading collection.bin..."
wrangler r2 object put "$R2_BUCKET/$R2_PREFIX/collection.bin" \
    --file "$BUILD_DIR/collection.bin" \
    --content-type "application/octet-stream" \
    --remote

# Upload sprites sequentially with progress (parallel was causing issues)
echo "   Uploading sprites ($SPRITE_COUNT files)..."
count=0
for file in "$BUILD_DIR"/sprites/*.webp; do
    basename=$(basename "$file")
    wrangler r2 object put "$R2_BUCKET/$R2_PREFIX/sprites/$basename" \
        --file "$file" \
        --content-type "image/webp" \
        --remote >/dev/null 2>&1
    count=$((count + 1))
    if [ $((count % 10)) -eq 0 ]; then
        echo -n "."
    fi
done
echo " done ($count files)"

# Upload HCF shards sequentially
echo "   Uploading HCF shards ($HCF_COUNT files)..."
count=0
for file in "$BUILD_DIR"/hcf/*.hcf; do
    basename=$(basename "$file")
    wrangler r2 object put "$R2_BUCKET/$R2_PREFIX/hcf/$basename" \
        --file "$file" \
        --content-type "application/octet-stream" \
        --remote >/dev/null 2>&1
    count=$((count + 1))
    echo -n "."
done
echo " done ($count files)"

echo ""
echo "Publishing complete!"
echo ""
echo "R2 locations:"
echo "   $R2_BUCKET/$R2_PREFIX/collection.bin"
echo "   $R2_BUCKET/$R2_PREFIX/sprites/ ($SPRITE_COUNT files)"
echo "   $R2_BUCKET/$R2_PREFIX/hcf/ ($HCF_COUNT files)"
echo ""
echo "Public URL (if bucket is public):"
echo "   https://files.hodlcroft.com/$R2_PREFIX/collection.bin"
