# Viewer CLI helpers

# Default recipe
default:
    @just --list

# Build the CLI
build:
    cargo build --package viewer-cli

# Build release
build-release:
    cargo build --package viewer-cli --release

# Run tests
test:
    cargo test

# Fetch and analyze a Cardano collection (dry run, no images)
fetch-cardano policy_id:
    cargo run --package viewer-cli -- fetch cardano {{ policy_id }}

# Sync a Cardano collection (full pipeline)
sync-cardano policy_id:
    cargo run --package viewer-cli -- sync cardano {{ policy_id }}

# Sync Cardano without images (metadata only)
sync-cardano-metadata policy_id:
    cargo run --package viewer-cli -- sync cardano {{ policy_id }} --skip-images

# Show bundle info
info path:
    cargo run --package viewer-cli -- info {{ path }}

# Clean build artifacts
clean:
    rm -rf .build/
    cargo clean

# Clean just the .build directory
clean-build:
    rm -rf .build/

# Watch and rebuild on changes
watch:
    cargo watch -x 'build --package viewer-cli'

# Run with debug logging
sync-cardano-debug policy_id:
    RUST_LOG=debug cargo run --package viewer-cli -- sync cardano {{ policy_id }}

# Run with trace logging (very verbose)
sync-cardano-trace policy_id:
    RUST_LOG=trace cargo run --package viewer-cli -- sync cardano {{ policy_id }}
