#!/bin/bash
set -euo pipefail

# Remove collection from Cloudflare R2
# Usage: ./scripts/unpublish.sh <slug>
#
# Example: ./scripts/unpublish.sh blackflag
#
# Removes from R2:
#   collections/<slug>/collection.bin
#   collections/<slug>/sprites/*
#   collections/<slug>/hcf/*
#
# Prerequisites:
#   - wrangler CLI installed and authenticated

# Parse arguments
if [ -z "${1:-}" ]; then
    echo "Usage: $0 <slug>"
    echo ""
    echo "Arguments:"
    echo "  slug    Collection slug (e.g., blackflag)"
    echo ""
    echo "Example:"
    echo "  $0 blackflag"
    exit 1
fi

SLUG="$1"

# Configuration
R2_BUCKET="${R2_BUCKET:-hodlcroft}"
R2_PREFIX="collections/$SLUG"

echo "Unpublishing collection from R2"
echo "   Slug:      $SLUG"
echo "   R2 Bucket: $R2_BUCKET"
echo "   R2 Path:   $R2_PREFIX/"
echo ""

# Confirm deletion
read -p "Are you sure you want to delete all files for '$SLUG'? [y/N] " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo "Aborted."
    exit 0
fi

echo ""
echo "Listing objects to delete..."

# List all objects with the prefix
OBJECTS=$(wrangler r2 object list "$R2_BUCKET" --prefix "$R2_PREFIX/" --remote 2>/dev/null | grep -o '"key":"[^"]*"' | sed 's/"key":"//;s/"$//' || true)

if [ -z "$OBJECTS" ]; then
    echo "No objects found at $R2_PREFIX/"
    exit 0
fi

# Count objects
OBJECT_COUNT=$(echo "$OBJECTS" | wc -l | tr -d ' ')
echo "Found $OBJECT_COUNT objects to delete"
echo ""

echo "Deleting objects..."
echo "$OBJECTS" | while read -r key; do
    if [ -n "$key" ]; then
        echo "   Deleting $key..."
        wrangler r2 object delete "$R2_BUCKET/$key" --remote
    fi
done

echo ""
echo "Unpublish complete!"
echo "Removed $OBJECT_COUNT objects from $R2_BUCKET/$R2_PREFIX/"
