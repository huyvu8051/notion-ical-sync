#!/bin/bash
# Format code and auto-fix clippy lints

echo "Formatting codebase..."
cargo fmt --all

echo "Running clippy auto-fixes..."
cargo clippy --fix --allow-dirty --allow-staged --all-targets

echo "Done!"
