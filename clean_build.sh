#!/bin/bash
# Completely clean build cache and old build artifacts

PROJECT_DIR="$(cd "$(dirname "$0")" && pwd)"

echo "🧹 Cleaning build cache..."
echo ""

# Clean iOS-related builds in target directory
echo "Removing iOS build targets..."
rm -rf "$PROJECT_DIR/target/aarch64-apple-ios"
rm -rf "$PROJECT_DIR/target/aarch64-apple-ios-sim"
rm -rf "$PROJECT_DIR/target/x86_64-apple-ios"

# Clean build directory
echo "Removing build directory..."
rm -rf "$PROJECT_DIR/build"

# Clean Cargo cache (optional, will re-download dependencies)
# rm -rf "$PROJECT_DIR/target"

echo ""
echo "✅ Cleanup completed!"
echo ""
echo "You can now run: ./build_ios.sh"

