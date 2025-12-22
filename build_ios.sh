#!/bin/bash
set -e

# =============================================================================
# Miden Swift Client iOS Build Script
# Build Rust static library and package as XCFramework
# =============================================================================

PROJECT_DIR="$(cd "$(dirname "$0")" && pwd)"
BUILD_DIR="$PROJECT_DIR/build"
IOS_DIR="$BUILD_DIR/ios"

# iOS minimum deployment version (can be overridden via environment variable)
IOS_DEPLOYMENT_TARGET="${IOS_DEPLOYMENT_TARGET:-18.5}"

echo "📦 Miden Swift Client iOS Build Script"
echo "========================================"
echo "iOS Deployment Target: $IOS_DEPLOYMENT_TARGET"
echo ""

# -----------------------------------------------------------------------------
# Step 0: Update Cargo config to use specified iOS version
# -----------------------------------------------------------------------------
echo "⚙️  Step 0: Updating Cargo config..."
mkdir -p "$PROJECT_DIR/.cargo"
cat > "$PROJECT_DIR/.cargo/config.toml" << EOF
# Cargo configuration file
# Auto-generated - Set minimum deployment version for iOS build targets

[target.aarch64-apple-ios]
# iOS device (arm64)
rustflags = [
    "-C", "link-arg=-miphoneos-version-min=$IOS_DEPLOYMENT_TARGET",
]

[target.aarch64-apple-ios-sim]
# iOS simulator (arm64, Apple Silicon)
rustflags = [
    "-C", "link-arg=-mios-simulator-version-min=$IOS_DEPLOYMENT_TARGET",
]

[target.x86_64-apple-ios]
# iOS simulator (x86_64, Intel Mac) - if support needed
rustflags = [
    "-C", "link-arg=-mios-simulator-version-min=$IOS_DEPLOYMENT_TARGET",
]
EOF
echo "   ✅ Cargo config updated"

# -----------------------------------------------------------------------------
# Step 1: Generate C header file
# -----------------------------------------------------------------------------
echo ""
echo "🔧 Step 1: Generating C header file..."
cbindgen --config cbindgen.toml --crate miden_swift_client --output miden_swift_client.h
echo "   ✅ miden_swift_client.h generated"

# -----------------------------------------------------------------------------
# Step 2: Build iOS device static library (arm64)
# -----------------------------------------------------------------------------
echo ""
echo "🔨 Step 2: Building iOS device (aarch64-apple-ios)..."
cargo build --release --target aarch64-apple-ios
echo "   ✅ iOS device build completed"

# -----------------------------------------------------------------------------
# Step 3: Build iOS simulator static library (arm64, Apple Silicon)
# -----------------------------------------------------------------------------
echo ""
echo "🔨 Step 3: Building iOS simulator (aarch64-apple-ios-sim)..."
cargo build --release --target aarch64-apple-ios-sim
echo "   ✅ iOS simulator build completed"

# -----------------------------------------------------------------------------
# Step 4: Prepare build directory
# -----------------------------------------------------------------------------
echo ""
echo "📁 Step 4: Preparing build directory..."

# Clean old builds
rm -rf "$BUILD_DIR"
mkdir -p "$IOS_DIR/arm64"
mkdir -p "$IOS_DIR/sim"

# Copy static libraries
cp "$PROJECT_DIR/target/aarch64-apple-ios/release/libmiden_swift_client.a" "$IOS_DIR/arm64/"
cp "$PROJECT_DIR/target/aarch64-apple-ios-sim/release/libmiden_swift_client.a" "$IOS_DIR/sim/"

# Copy header file
cp "$PROJECT_DIR/miden_swift_client.h" "$IOS_DIR/"

# Create module.modulemap
cat > "$IOS_DIR/module.modulemap" << EOF
module MidenSwiftClient {
    header "miden_swift_client.h"
    export *
}
EOF

echo "   ✅ Build directory prepared"

# -----------------------------------------------------------------------------
# Step 5: Create XCFramework
# -----------------------------------------------------------------------------
echo ""
echo "📱 Step 5: Creating XCFramework..."

xcodebuild -create-xcframework \
    -library "$IOS_DIR/arm64/libmiden_swift_client.a" -headers "$IOS_DIR" \
    -library "$IOS_DIR/sim/libmiden_swift_client.a" -headers "$IOS_DIR" \
    -output "$BUILD_DIR/miden_swift_client.xcframework"

echo "   ✅ XCFramework created"

# -----------------------------------------------------------------------------
# Done
# -----------------------------------------------------------------------------
echo ""
echo "================================"
echo "🎉 Build completed!"
echo ""
echo "Output file:"
echo "  - $BUILD_DIR/miden_swift_client.xcframework"
echo ""
echo "Usage:"
echo "  1. Drag miden_swift_client.xcframework into your Xcode project"
echo "  2. Add to Build Phases > Link Binary With Libraries"
echo "  3. Import: import MidenSwiftClient"
echo ""
echo "Custom iOS version:"
echo "  IOS_DEPLOYMENT_TARGET=18.5 ./build_ios.sh"
echo ""

