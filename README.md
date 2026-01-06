# Miden Swift Client SDK

A Swift SDK for interacting with the Miden blockchain, built on top of the Rust `miden-client` library via FFI.

## Features

- ✅ **Account Management**: Create and manage Miden accounts
- ✅ **State Synchronization**: Sync with Miden network
- ✅ **Balance Queries**: Get account balances and asset information
- ✅ **Note Management**: Get consumable notes and consume them
- ✅ **Transaction Submission**: Submit transactions to the network
- ✅ **iOS Ready**: Pre-built XCFramework for easy iOS integration

## Requirements

- iOS 26.1+ / macOS 15.6+
- Xcode 26.1+
- Rust 1.91+ (for building from source)

## Installation

### Using XCFramework

1. Build the XCFramework:

   ```bash
   ./build_ios.sh
   ```

2. Add `miden_swift_client.xcframework` to your Xcode project:

   - Drag the framework into your project
   - Add to "Frameworks, Libraries, and Embedded Content"
   - Set "Embed" to "Embed & Sign"

3. Copy `MidenWallet.swift` to your project

4. Import the module:
   ```swift
   import MidenSwiftClient
   ```

## Quick Start

```swift
import Foundation
import MidenSwiftClient

// Initialize wallet
let wallet = try MidenWallet()

// Sync with network
let blockNum = try wallet.sync()
print("Synced to block: \(blockNum)")

// Create a new account
let accountId = try wallet.createWallet()
print("Created account: \(accountId)")

// Get account balance
let balance = try wallet.getBalance(accountId: accountId)
print("Balance: \(balance)")

// Get consumable notes
let notes = try wallet.getInputNotes(accountId: accountId)
print("Found \(notes.totalCount) consumable notes")

// Consume notes
if !notes.consumableNoteIds.isEmpty {
    let txId = try wallet.consumeNotes(
        accountId: accountId,
        noteIds: notes.consumableNoteIds
    )
    print("Transaction submitted: \(txId)")
}
```

## SwiftUI Example

Here's a complete SwiftUI example demonstrating how to use the SDK:

```swift
struct ContentView: View {
    @State private var wallet: MidenWallet?
    var body: some View {
        VStack {
            VStack {
                Button("Create new") {
                    Task {
                        let newAccount = try wallet?.createWallet()
                        print("new account: \(newAccount ?? "")")
                        let accounts = try wallet?.getAccounts()
                        print("accounts: \(accounts ?? [])")
                    }
                }
                Button("Get Notes") {
                    Task {
                        do {
                            guard let wallet = wallet else {
                                return
                            }
                            let res = try wallet.sync()
                            print("res:\(String(describing: res))")
                            let accounts = try wallet.getAccounts()
                            print("accounts: \(accounts)")
                            if accounts.count == 0 {
                                return
                            }
                            let account = accounts[0]

                            let notes = try wallet.getInputNotes(accountId: account)
                            print("Found \(notes.totalCount) consumable notes")

                            for note in (notes.notes) {
                                print("Note: \(note.noteId)")
                                for asset in note.assets {
                                    print("  - \(asset.amount) from \(asset.faucetId)")
                                }
                            }

                            _ = try wallet.sync()
                            if !notes.consumableNoteIds.isEmpty {
                                let txId = try wallet.consumeNotes(
                                    accountId: account,
                                    noteIds: notes.consumableNoteIds
                                )
                                print("Transaction submitted: \(txId)")
                            }
                        } catch {
                            print("An unexpected error occurred: \(error)")
                        }
                    }
                }
                Button("Get Balance") {
                    Task {
                        do {
                            guard let wallet = wallet else {
                                return
                            }
                            _ = try wallet.sync()
                            let accounts = try wallet.getAccounts()
                            if accounts.count == 0 {
                                return
                            }
                            let account = accounts[0]
                            let balance = try wallet.getBalance(accountId: account)
                            print("Total fungible: \(balance.totalFungibleCount)")

                            for asset in balance.fungibleAssets {
                                print("Faucet: \(asset.faucetId), Amount: \(asset.amount)")
                            }
                        } catch {
                            print("error")
                        }
                    }
                }

            }
        }
        .padding()
        .onAppear {
            do {
                wallet = try MidenWallet()
                print("MidenWallet created successfully")
                print("Keystore ready: \(wallet!.isKeystoreReady)")  // true
                print("Store ready: \(wallet!.isStoreReady)")        // true
                print("Keystore path: \(wallet!.keystoreDirectory)")
                print("Store path: \(wallet!.storeFile)")
            } catch {
                print("Failed to create MidenWallet: \(error)")
            }
        }
    }
}
```

## API Reference

### MidenWallet

#### Initialization

```swift
public init(
    keystorePath: String? = nil,
    storePath: String? = nil,
    rpcEndpoint: String? = nil
) throws
```

#### Methods

- `sync() throws -> UInt32` - Sync state with network
- `createWallet(seed: [UInt8]? = nil) throws -> String` - Create new account
- `getAccounts() throws -> [String]` - Get all account IDs
- `getBalance(accountId: String) throws -> AccountBalance` - Get account balance
- `getInputNotes(accountId: String? = nil) throws -> InputNotesResult` - Get consumable notes
- `consumeNotes(accountId: String, noteIds: [String]) throws -> String` - Consume notes
- `testConnection() throws -> Bool` - Test network connection

## Building from Source

### Prerequisites

- Rust toolchain (install via [rustup](https://rustup.rs/))
- iOS toolchain
- `cbindgen` (installed automatically via Cargo)

### Build Steps

1. Clone the repository:

   ```bash
   git clone <repository-url>
   cd miden-swift-client
   ```

2. Build for iOS:
   ```bash
   ./build_ios.sh
   ```

This will:

- Generate C header files using `cbindgen`
- Build static libraries for iOS (arm64) and iOS Simulator (arm64-sim)
- Create an XCFramework

### Build Configuration

The build script uses the following iOS deployment targets:

- iOS Device: 18.5
- iOS Simulator: 18.5

You can modify these in `.cargo/config.toml` and `build_ios.sh`.

## Project Structure

```
miden-swift-client/
├── src/
│   └── lib.rs              # Rust FFI implementation
├── MidenWallet.swift       # Swift wrapper class
├── miden_swift_client.h    # C header file (auto-generated)
├── build_ios.sh            # iOS build script
├── Cargo.toml              # Rust dependencies
└── README.md               # This file
```

## Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│  Swift                                                           │
│  ┌────────────────────────────┐                                  │
│  │ wc_miden_sync_async()      │───┐                              │
│  │ wc_miden_get_balance()     │   │                              │
│  │ ...                        │   │                              │
│  └────────────────────────────┘   │                              │
└───────────────────────────────────│──────────────────────────────┘
                                    │ mpsc::UnboundedSender<Request>
                                    ▼
┌─────────────────────────────────────────────────────────────────┐
│  Rust Worker Thread (Tokio runtime)                             │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                     Event Loop                           │   │
│  │  ┌───────────────┐    ┌─────────────────────────────┐    │   │
│  │  │ recv Request  │───▶│ match request {             │    │   │
│  │  └───────────────┘    │   SyncSync => ...           │    │   │
│  │                       │   GetBalanceAsync => ...    │    │   │
│  │                       │   Shutdown => break         │    │   │
│  │                       │ }                           │    │   │
│  │                       └─────────────────────────────┘    │   │
│  │                                                          │   │
│  │  ┌─────────────────────────────────────────────────┐     │   │
│  │  │ MidenContext                                    │     │   │
│  │  │   - client: MidenClient                         │     │   │
│  │  │   - keystore: Arc<MidenKeyStore>                │     │   │
│  │  └─────────────────────────────────────────────────┘     │   │
│  └──────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

## Error Handling

All methods throw `MidenError` which provides detailed error information:

```swift
do {
    let wallet = try MidenWallet()
    let res = try wallet.sync()
    print("res:\(String(describing: res))")
} catch let error as MidenError {
    print("Error: \(error.localizedDescription)")
} catch {
    print("Unexpected error: \(error)")
}
```

## Account Storage Modes

The SDK supports both **Public** and **Private** account storage modes:

- **Public**: Account state is stored on-chain (visible to everyone)
- **Private**: Only account hash is stored on-chain (privacy-preserving)

**Note**: Private accounts require special handling for the first transaction. Currently, the SDK creates Public accounts by default for easier integration.

## Thread Safety

The SDK is thread-safe. The underlying Rust runtime manages a global Tokio runtime that handles all async operations.

## Limitations

- Currently supports testnet only
- Private account deployment requires additional setup
- Some advanced features from `miden-client` are not yet exposed

## Plans

### Short-term

- [ ] Private account support
- [ ] Additional transaction types (transfers, P2ID notes)
- [ ] Swift Package Manager distribution

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

Built on top of the [Miden Client](https://github.com/0xPolygonMiden/miden-client) Rust library.

## Support

For issues and questions, please open an issue on GitHub.
