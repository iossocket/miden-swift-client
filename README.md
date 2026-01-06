# Miden Swift Client SDK

A Swift SDK for interacting with the Miden blockchain, built on top of the Rust `miden-client` library via FFI.

## Features

- ✅ **Account Management**: Create and manage Miden accounts
- ✅ **State Synchronization**: Sync with Miden network
- ✅ **Balance Queries**: Get account balances and asset information
- ✅ **Note Management**: Get consumable notes and consume them
- ✅ **Transaction Submission**: Submit transactions to the network
- ✅ **Async/Await Support**: Non-blocking async APIs for Swift concurrency
- ✅ **Thread-Safe**: Worker thread architecture with bounded queue
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

### Using Async APIs (Recommended for UI)

```swift
import Foundation
import MidenSwiftClient

Task {
    do {
        // Initialize wallet
        let wallet = try MidenWallet()

        // Sync with network (non-blocking)
        let blockNum = try await wallet.syncAsync()
        print("Synced to block: \(blockNum)")

        // Create a new account
        let accountId = try await wallet.createWalletAsync()
        print("Created account: \(accountId)")

        // Get account balance
        let balance = try await wallet.getBalanceAsync(accountId: accountId)
        print("Balance: \(balance)")

        // Get consumable notes
        let notes = try await wallet.getInputNotesAsync(accountId: accountId)
        print("Found \(notes.totalCount) consumable notes")

        // Consume notes
        if !notes.consumableNoteIds.isEmpty {
            let txId = try await wallet.consumeNotesAsync(
                accountId: accountId,
                noteIds: notes.consumableNoteIds
            )
            print("Transaction submitted: \(txId)")
        }
    } catch {
        print("Error: \(error)")
    }
}
```

### Using Synchronous APIs (Background Thread Only)

```swift
Task.detached {
    do {
        let wallet = try MidenWallet()

        // ⚠️ These are blocking calls - only use from background threads
        let blockNum = try wallet.sync()
        let accountId = try wallet.createWallet()
        let balance = try wallet.getBalance(accountId: accountId)

        // Update UI on main thread
        await MainActor.run {
            // Update UI with results
        }
    } catch {
        print("Error: \(error)")
    }
}
```

## SwiftUI Example

Here's a complete SwiftUI example demonstrating how to use the SDK with async APIs:

```swift
import SwiftUI
import MidenSwiftClient

struct ContentView: View {
    @State private var wallet: MidenWallet?
    @State private var accounts: [String] = []
    @State private var balance: AccountBalance?
    @State private var errorMessage: String?

    var body: some View {
        VStack(spacing: 20) {
            if let error = errorMessage {
                Text("Error: \(error)")
                    .foregroundColor(.red)
            }

            Button("Sync") {
                Task {
                    do {
                        guard let wallet = wallet else { return }
                        let blockNum = try await wallet.syncAsync()
                        print("Synced to block: \(blockNum)")
                    } catch {
                        errorMessage = error.localizedDescription
                    }
                }
            }

            Button("Create New Account") {
                Task {
                    do {
                        guard let wallet = wallet else { return }
                        let newAccount = try await wallet.createWalletAsync()
                        print("New account: \(newAccount)")
                        await refreshAccounts()
                    } catch {
                        errorMessage = error.localizedDescription
                    }
                }
            }

            Button("Get Balance") {
                Task {
                    do {
                        guard let wallet = wallet,
                              let account = accounts.first else { return }
                        balance = try await wallet.getBalanceAsync(accountId: account)
                        print("Total fungible: \(balance?.totalFungibleCount ?? 0)")
                    } catch {
                        errorMessage = error.localizedDescription
                    }
                }
            }

            Button("Get Notes & Consume") {
                Task {
                    do {
                        guard let wallet = wallet,
                              let account = accounts.first else { return }

                        let notes = try await wallet.getInputNotesAsync(accountId: account)
                        print("Found \(notes.totalCount) consumable notes")

                        if !notes.consumableNoteIds.isEmpty {
                            let txId = try await wallet.consumeNotesAsync(
                                accountId: account,
                                noteIds: notes.consumableNoteIds
                            )
                            print("Transaction submitted: \(txId)")
                        }
                    } catch {
                        errorMessage = error.localizedDescription
                    }
                }
            }
        }
        .padding()
        .onAppear {
            do {
                wallet = try MidenWallet()
                print("MidenWallet created successfully")
                Task {
                    await refreshAccounts()
                }
            } catch {
                errorMessage = "Failed to create wallet: \(error)"
            }
        }
    }

    private func refreshAccounts() async {
        do {
            guard let wallet = wallet else { return }
            accounts = try await wallet.getAccountsAsync()
        } catch {
            errorMessage = error.localizedDescription
        }
    }
}
```

**Note**: All async methods (`syncAsync()`, `getBalanceAsync()`, etc.) are safe to call from the main thread and will not block the UI.

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

**Synchronous (Blocking) - ⚠️ Do NOT call from main/UI thread:**

- `sync() throws -> UInt32` - Sync state with network (blocks up to 30s)
- `createWallet(seed: [UInt8]? = nil) throws -> String` - Create new account
- `getAccounts() throws -> [String]` - Get all account IDs
- `getBalance(accountId: String) throws -> AccountBalance` - Get account balance
- `getInputNotes(accountId: String? = nil) throws -> InputNotesResult` - Get consumable notes
- `consumeNotes(accountId: String, noteIds: [String]) throws -> String` - Consume notes
- `testConnection() throws -> Bool` - Test network connection

**Asynchronous (Non-blocking) - ✅ Recommended for UI:**

- `syncAsync() async throws -> UInt32` - Sync state with network
- `createWalletAsync(seed: [UInt8]? = nil) async throws -> String` - Create new account
- `getAccountsAsync() async throws -> [String]` - Get all account IDs
- `getBalanceAsync(accountId: String) async throws -> AccountBalance` - Get account balance
- `getInputNotesAsync(accountId: String? = nil) async throws -> InputNotesResult` - Get consumable notes
- `consumeNotesAsync(accountId: String, noteIds: [String]) async throws -> String` - Consume notes
- `testConnectionAsync() async throws -> Bool` - Test network connection

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

The SDK uses a **worker thread architecture** to ensure thread safety and prevent blocking the main thread:

```
┌──────────────────────────────────────────────────────────────────┐
│  Swift (Main Thread / Background Threads)                        │
│  ┌────────────────────────────┐                                  │
│  │ syncAsync()                │───┐                              │
│  │ getBalanceAsync()          │   │                              │
│  │ sync() [blocking]          │   │                              │
│  │ ...                        │   │                              │
│  └────────────────────────────┘   │                              │
└───────────────────────────────────│──────────────────────────────┘
                                    │ mpsc::Sender<Request>
                                    │ (bounded queue: 256)
                                    ▼
┌─────────────────────────────────────────────────────────────────┐
│  Rust Worker Thread (Single-threaded Tokio runtime)             │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                     Event Loop                           │   │
│  │  ┌───────────────┐    ┌─────────────────────────────┐    │   │
│  │  │ recv Request  │───▶│ match request {             │    │   │
│  │  │ (sequential)  │    │   SyncSync => ...           │    │   │
│  │  └───────────────┘    │   GetBalanceAsync => ...    │    │   │
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

### Key Design Decisions

1. **Worker Thread**: All `MidenClient` operations run in a dedicated worker thread with a single-threaded Tokio runtime. This avoids `Send` trait requirements and ensures thread safety.

2. **Bounded Queue**: Request queue has a capacity of 256. If the queue is full, new requests return `ERR_QUEUE_FULL (-8)`.

3. **Fast Shutdown**: `wc_miden_destroy` performs a fast shutdown - pending requests in the queue are dropped, and callbacks for in-flight async operations may not be invoked.

4. **Memory Management**: Async callbacks return data via `wc_bytes_free` - Swift must call this to release Rust-allocated memory.

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

### Error Codes

The SDK uses standardized error codes:

| Code | Constant             | Meaning                                                            |
| ---- | -------------------- | ------------------------------------------------------------------ |
| 0    | -                    | Success                                                            |
| -1   | `ERR_INVALID_PARAM`  | Invalid parameter (null pointer, invalid format, buffer too small) |
| -2   | `ERR_INVALID_HANDLE` | Invalid handle or worker closed                                    |
| -3   | `ERR_ACCOUNT_OP`     | Account/key operation failed                                       |
| -4   | `ERR_NOTE_OP`        | Note operation failed / invalid note ID                            |
| -5   | `ERR_LOOKUP`         | Balance/account lookup failed                                      |
| -6   | `ERR_TX_SUBMIT`      | Transaction submission failed                                      |
| -8   | `ERR_QUEUE_FULL`     | Worker queue is full (too many pending requests)                   |
| -99  | `ERR_TIMEOUT`        | Operation timed out (sync API only, 30s timeout)                   |

**Note**: Timeout (-99) only abandons waiting; the operation may still complete in the background.

## Memory Management

The SDK uses Rust-allocated memory for async callback results. The Swift wrapper (`MidenWallet`) automatically manages this for you, but if you're using the C FFI directly:

**Important**: You MUST call `wc_bytes_free(ptr, len)` to release memory returned by async callbacks:

```c
// In your callback:
void callback(void* user_data, int32_t error_code, uint8_t* data_ptr, uintptr_t data_len) {
    if (error_code == 0 && data_ptr != NULL) {
        // Use the data...

        // MUST free the memory:
        wc_bytes_free(data_ptr, data_len);
    }
}
```

The Swift wrapper handles this automatically - you don't need to call `wc_bytes_free` when using `MidenWallet` class methods.

## Account Storage Modes

The SDK supports both **Public** and **Private** account storage modes:

- **Public**: Account state is stored on-chain (visible to everyone)
- **Private**: Only account hash is stored on-chain (privacy-preserving)

**Note**: Private accounts require special handling for the first transaction. Currently, the SDK creates Public accounts by default for easier integration.

## Thread Safety & Concurrency

### Thread Safety

The SDK is thread-safe. All operations are serialized through a dedicated worker thread, ensuring no race conditions.

### Callback Thread Context

⚠️ **Important**: Async callbacks are invoked on the **worker thread**, NOT the main thread.

If you need to update UI from a callback, dispatch to the main queue:

```swift
let balance = try await wallet.getBalanceAsync(accountId: accountId)
// balance is received on worker thread

// If updating UI, dispatch to main thread:
DispatchQueue.main.async {
    self.balanceLabel.text = "\(balance.totalFungibleCount)"
}
```

The Swift wrapper (`MidenWallet`) automatically handles this for you - all async methods return on the calling thread (typically the main thread when used with SwiftUI).

### Synchronous vs Asynchronous APIs

**Synchronous APIs** (e.g., `sync()`, `getBalance()`):

- ⚠️ **Blocking**: Will block the calling thread for up to 30 seconds
- ⚠️ **Do NOT call from main/UI thread** - will freeze the UI
- ✅ Use only from background threads
- Returns `ERR_TIMEOUT (-99)` if operation exceeds 30 seconds

**Asynchronous APIs** (e.g., `syncAsync()`, `getBalanceAsync()`):

- ✅ **Non-blocking**: Returns immediately, uses callbacks
- ✅ **Safe to call from main thread**
- ✅ **Recommended for UI applications**
- Callbacks are invoked on worker thread (Swift wrapper handles main thread dispatch)

### Best Practices

1. **Prefer async APIs** for UI applications:

   ```swift
   // ✅ Good - non-blocking
   let balance = try await wallet.getBalanceAsync(accountId: accountId)

   // ❌ Bad - blocks UI thread
   let balance = try wallet.getBalance(accountId: accountId)
   ```

2. **If using sync APIs**, ensure you're on a background thread:

   ```swift
   Task.detached {
       let balance = try wallet.getBalance(accountId: accountId)
       await MainActor.run {
           // Update UI
       }
   }
   ```

3. **Handle queue full errors**:
   ```swift
   do {
       try await wallet.syncAsync()
   } catch MidenError.queueFull {
       // Too many pending requests, retry later
   }
   ```

## Limitations

- Currently supports testnet only
- Private account deployment requires additional setup
- Some advanced features from `miden-client` are not yet exposed
- Worker queue capacity: 256 requests (returns `ERR_QUEUE_FULL` when full)
- Synchronous API timeout: 30 seconds (returns `ERR_TIMEOUT` if exceeded)
- Fast shutdown: `destroy()` drops pending requests (does not wait for completion)
- Callbacks execute on worker thread (not main thread) - Swift wrapper handles dispatch

## Resource Management

The `MidenWallet` class automatically manages resources. When the instance is deallocated, it calls `wc_miden_destroy()` which:

1. Sends a shutdown signal to the worker thread
2. Drops the sender (closing the channel)
3. Waits for the worker thread to finish

**Note**: This is a **fast shutdown** - pending requests in the queue are dropped, and callbacks for in-flight async operations may not be invoked.

If you need to ensure all operations complete before shutdown, wait for all pending async operations to finish before deallocating the wallet instance.

## Plans

### Short-term

- [ ] Private account support
- [ ] Additional transaction types (transfers, P2ID notes)
- [ ] Swift Package Manager distribution
- [ ] Graceful shutdown option (drain queue before exit)

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

Built on top of the [Miden Client](https://github.com/0xPolygonMiden/miden-client) Rust library.

## Support

For issues and questions, please open an issue on GitHub.
