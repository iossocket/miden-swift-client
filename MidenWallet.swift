//
//  MidenWallet.swift
//  Miden Swift Client Swift Wrapper
//
//  A Swift wrapper class that simplifies the use of Miden Client
//  Can be directly copied to your Xcode project
//

import Foundation
import MidenSwiftClient

/// Miden Wallet Manager
///
/// Usage example:
/// ```swift
/// let wallet = try MidenWallet()
/// try wallet.sync()
/// let accountId = try wallet.createWallet()
/// ```
public class MidenWallet {
    
    // MARK: - Properties
    
    private var handle: OpaquePointer?
    private let keystorePath: String
    private let storePath: String

    var isKeystoreReady: Bool {
        FileManager.default.fileExists(atPath: keystorePath)
    }
    
    var isStoreReady: Bool {
        FileManager.default.fileExists(atPath: storePath)
    }
    
    var isReady: Bool {
        handle != nil && isKeystoreReady && isStoreReady
    }
    
    var keystoreDirectory: String {
        keystorePath
    }
    
    var storeFile: String {
        storePath
    }
    
    // MARK: - Initialization
    
    /// Initialize Miden Wallet
    ///
    /// - Parameters:
    ///   - keystorePath: Keystore storage directory path (optional, defaults to Documents/miden_keystore)
    ///   - storePath: SQLite database file path (optional, defaults to Documents/miden_store.sqlite3)
    ///   - rpcEndpoint: RPC endpoint URL (optional, nil uses testnet)
    /// - Throws: If initialization fails
    public init(
        keystorePath: String? = nil,
        storePath: String? = nil,
        rpcEndpoint: String? = nil
    ) throws {
        // Get Documents directory
        let documentsPath = FileManager.default.urls(
            for: .documentDirectory,
            in: .userDomainMask
        ).first!
        
        // Set default paths
        self.keystorePath = keystorePath ?? documentsPath
            .appendingPathComponent("miden_keystore")
            .path
        
        self.storePath = storePath ?? documentsPath
            .appendingPathComponent("miden_store.sqlite3")
            .path
        
        // Create keystore directory if it doesn't exist
        let keystoreURL = URL(fileURLWithPath: self.keystorePath)
        try? FileManager.default.createDirectory(
            at: keystoreURL,
            withIntermediateDirectories: true,
            attributes: nil
        )
        
        // Create client
        var handlePtr: OpaquePointer?
        let result = self.keystorePath.withCString { ks in
            self.storePath.withCString { store in
                if let endpoint = rpcEndpoint {
                    return endpoint.withCString { ep in
                        wc_miden_create(ks, store, ep, &handlePtr)
                    }
                } else {
                    return wc_miden_create(ks, store, nil, &handlePtr)
                }
            }
        }
        
        guard result == 0, let h = handlePtr else {
            throw MidenError.initializationFailed(code: result)
        }
        
        self.handle = h
    }
    
    deinit {
        if let h = handle {
            wc_miden_destroy(h)
        }
    }
    
    // MARK: - Public Methods
    
    /// Sync blockchain state
    ///
    /// - Returns: Latest block number
    /// - Throws: If sync fails
    public func sync() throws -> UInt32 {
        guard let h = handle else {
            throw MidenError.invalidHandle
        }
        
        var blockNum: UInt32 = 0
        let result = wc_miden_sync(h, &blockNum)
        
        guard result == 0 else {
            throw MidenError.syncFailed(code: result)
        }
        
        return blockNum
    }
    
    /// Create a new wallet account
    ///
    /// - Parameter seed: 32-byte seed (optional, nil auto-generates)
    /// - Returns: Account ID (hex string)
    /// - Throws: If creation fails
    public func createWallet(seed: [UInt8]? = nil) throws -> String {
        guard let h = handle else {
            throw MidenError.invalidHandle
        }
        
        // Prepare seed
        let seedPtr: UnsafePointer<UInt8>?
        let seedLen: Int
        
        if let seed = seed {
            guard seed.count == 32 else {
                throw MidenError.invalidSeedLength
            }
            seedPtr = seed.withUnsafeBytes { $0.baseAddress?.assumingMemoryBound(to: UInt8.self) }
            seedLen = 32
        } else {
            seedPtr = nil
            seedLen = 0
        }
        
        // Create account
        var accountIdBuffer = [UInt8](repeating: 0, count: 64)
        var accountIdLen: Int = 64
        
        let result = wc_miden_create_wallet(
            h,
            seedPtr,
            UInt(seedLen),
            &accountIdBuffer,
            &accountIdLen
        )
        
        guard result == 0 else {
            throw MidenError.createWalletFailed(code: result)
        }
        
        guard let accountIdString = String(
            bytes: accountIdBuffer.prefix(accountIdLen),
            encoding: .utf8
        ) else {
            throw MidenError.invalidAccountId
        }
        
        return accountIdString
    }
    
    /// Get all accounts list
    ///
    /// - Returns: Array of account IDs
    /// - Throws: If retrieval fails
    public func getAccounts() throws -> [String] {
        guard let h = handle else {
            throw MidenError.invalidHandle
        }
        
        var jsonBuffer = [UInt8](repeating: 0, count: 4096)
        var jsonLen: Int = 4096
        
        let result = wc_miden_get_accounts(h, &jsonBuffer, &jsonLen)
        
        guard result == 0 else {
            throw MidenError.getAccountsFailed(code: result)
        }
        
        guard let jsonString = String(
            bytes: jsonBuffer.prefix(jsonLen),
            encoding: .utf8
        ) else {
            throw MidenError.invalidJSON
        }
        
        guard let data = jsonString.data(using: .utf8) else {
            throw MidenError.invalidJSON
        }
        
        do {
            let accountIds = try JSONDecoder().decode([String].self, from: data)
            return accountIds
        } catch {
            throw MidenError.jsonDecodeFailed(error: error)
        }
    }
    
    /// Get account balance
    ///
    /// - Parameter accountId: Account ID (hex string)
    /// - Returns: Account balance information
    /// - Throws: If retrieval fails
    public func getBalance(accountId: String) throws -> AccountBalance {
        guard let h = handle else {
            throw MidenError.invalidHandle
        }
        
        var jsonBuffer = [UInt8](repeating: 0, count: 8192)
        var jsonLen: Int = 8192
        
        let result = accountId.withCString { accountIdPtr in
            wc_miden_get_balance(h, accountIdPtr, &jsonBuffer, &jsonLen)
        }
        
        switch result {
        case 0:
            break
        case -3:
            throw MidenError.invalidAccountId
        case -4:
            throw MidenError.accountNotFound(accountId: accountId)
        default:
            throw MidenError.getBalanceFailed(code: result)
        }
        
        guard let jsonString = String(
            bytes: jsonBuffer.prefix(jsonLen),
            encoding: .utf8
        ) else {
            throw MidenError.invalidJSON
        }
        
        guard let data = jsonString.data(using: .utf8) else {
            throw MidenError.invalidJSON
        }
        
        do {
            let balance = try JSONDecoder().decode(AccountBalance.self, from: data)
            return balance
        } catch {
            throw MidenError.jsonDecodeFailed(error: error)
        }
    }
    
    /// Test network connection
    ///
    /// - Returns: Whether connection succeeded
    /// - Throws: If test fails
    public func testConnection() throws -> Bool {
        guard let h = handle else {
            throw MidenError.invalidHandle
        }
        
        let result = wc_miden_test_connection(h)
        
        guard result == 0 else {
            throw MidenError.connectionTestFailed(code: result)
        }
        
        return true
    }
    
    /// Convert account ID bytes to hex string
    ///
    /// - Parameter accountIdBytes: Account ID byte array
    /// - Returns: Hex string
    /// - Throws: If conversion fails
    public static func accountIdToHex(_ accountIdBytes: [UInt8]) throws -> String {
        var hexBuffer = [UInt8](repeating: 0, count: 128)
        var hexLen: Int = 128
        
        let result = accountIdBytes.withUnsafeBytes { bytes in
            guard let baseAddress = bytes.baseAddress?.assumingMemoryBound(to: UInt8.self) else {
                return -1
            }
            return Int(wc_miden_account_id_to_hex(
                baseAddress,
                UInt(accountIdBytes.count),
                &hexBuffer,
                &hexLen
            ))
        }
        
        guard result == 0 else {
            throw MidenError.hexConversionFailed(code: Int32(result))
        }
        
        guard let hexString = String(
            bytes: hexBuffer.prefix(hexLen),
            encoding: .utf8
        ) else {
            throw MidenError.invalidHexString
        }
        
        return hexString
    }
    
    /// Get consumable Input Notes
    ///
    /// - Parameter accountId: Account ID (optional, nil gets notes for all accounts)
    /// - Returns: List of consumable notes
    /// - Throws: If retrieval fails
    public func getInputNotes(accountId: String? = nil) throws -> InputNotesResult {
        guard let h = handle else {
            throw MidenError.invalidHandle
        }
        
        var jsonBuffer = [UInt8](repeating: 0, count: 16384)
        var jsonLen: Int = 16384
        
        let result: Int32
        if let accountId = accountId {
            result = accountId.withCString { accountIdPtr in
                wc_miden_get_input_notes(h, accountIdPtr, &jsonBuffer, &jsonLen)
            }
        } else {
            result = wc_miden_get_input_notes(h, nil, &jsonBuffer, &jsonLen)
        }
        
        switch result {
        case 0:
            break
        case -3:
            throw MidenError.invalidAccountId
        default:
            throw MidenError.getInputNotesFailed(code: result)
        }
        
        guard let jsonString = String(
            bytes: jsonBuffer.prefix(jsonLen),
            encoding: .utf8
        ) else {
            throw MidenError.invalidJSON
        }
        
        guard let data = jsonString.data(using: .utf8) else {
            throw MidenError.invalidJSON
        }
        
        do {
            let notesResult = try JSONDecoder().decode(InputNotesResult.self, from: data)
            return notesResult
        } catch {
            throw MidenError.jsonDecodeFailed(error: error)
        }
    }
    
    /// Consume Notes
    ///
    /// Create and submit a transaction to consume specified notes.
    ///
    /// - Parameters:
    ///   - accountId: Account ID to execute transaction
    ///   - noteIds: Array of note IDs to consume
    /// - Returns: Transaction ID
    /// - Throws: If consumption fails
    public func consumeNotes(accountId: String, noteIds: [String]) throws -> String {
        guard let h = handle else {
            throw MidenError.invalidHandle
        }
        
        guard !noteIds.isEmpty else {
            throw MidenError.emptyNoteIds
        }
        
        // Build JSON array
        let noteIdsJson = "[" + noteIds.map { "\"\($0)\"" }.joined(separator: ",") + "]"
        
        var txIdBuffer = [UInt8](repeating: 0, count: 128)
        var txIdLen: Int = 128
        
        let result = accountId.withCString { accountIdPtr in
            noteIdsJson.withCString { noteIdsPtr in
                wc_miden_consume_notes(h, accountIdPtr, noteIdsPtr, &txIdBuffer, &txIdLen)
            }
        }
        
        switch result {
        case 0:
            break
        case -3:
            throw MidenError.invalidAccountId
        case -4:
            throw MidenError.invalidNoteId
        case -5:
            throw MidenError.consumeNotesFailed(code: result, message: "Transaction creation failed")
        case -6:
            throw MidenError.consumeNotesFailed(code: result, message: "Transaction submission failed")
        default:
            throw MidenError.consumeNotesFailed(code: result, message: nil)
        }
        
        guard let txIdString = String(
            bytes: txIdBuffer.prefix(txIdLen),
            encoding: .utf8
        ) else {
            throw MidenError.invalidJSON
        }
        
        return txIdString
    }
}

// MARK: - Error Types

/// Miden Wallet error type
public enum MidenError: LocalizedError {
    case initializationFailed(code: Int32)
    case invalidHandle
    case syncFailed(code: Int32)
    case createWalletFailed(code: Int32)
    case getAccountsFailed(code: Int32)
    case getBalanceFailed(code: Int32)
    case getInputNotesFailed(code: Int32)
    case consumeNotesFailed(code: Int32, message: String?)
    case accountNotFound(accountId: String)
    case invalidSeedLength
    case invalidAccountId
    case invalidNoteId
    case emptyNoteIds
    case invalidJSON
    case jsonDecodeFailed(error: Error)
    case connectionTestFailed(code: Int32)
    case hexConversionFailed(code: Int32)
    case invalidHexString
    
    public var errorDescription: String? {
        switch self {
        case .initializationFailed(let code):
            return "Initialization failed (error code: \(code))"
        case .invalidHandle:
            return "Invalid client handle"
        case .syncFailed(let code):
            return "Sync failed (error code: \(code))"
        case .createWalletFailed(let code):
            return "Wallet creation failed (error code: \(code))"
        case .getAccountsFailed(let code):
            return "Get accounts failed (error code: \(code))"
        case .getBalanceFailed(let code):
            return "Get balance failed (error code: \(code))"
        case .getInputNotesFailed(let code):
            return "Get Input Notes failed (error code: \(code))"
        case .consumeNotesFailed(let code, let message):
            if let msg = message {
                return "Consume Notes failed: \(msg) (error code: \(code))"
            }
            return "Consume Notes failed (error code: \(code))"
        case .accountNotFound(let accountId):
            return "Account not found: \(accountId)"
        case .invalidSeedLength:
            return "Seed length must be 32 bytes"
        case .invalidAccountId:
            return "Invalid account ID"
        case .invalidNoteId:
            return "Invalid Note ID"
        case .emptyNoteIds:
            return "Note IDs cannot be empty"
        case .invalidJSON:
            return "Invalid JSON data"
        case .jsonDecodeFailed(let error):
            return "JSON decode failed: \(error.localizedDescription)"
        case .connectionTestFailed(let code):
            return "Connection test failed (error code: \(code))"
        case .hexConversionFailed(let code):
            return "Hex conversion failed (error code: \(code))"
        case .invalidHexString:
            return "Invalid hex string"
        }
    }
}

// MARK: - Data Models

/// Fungible asset information
public struct FungibleAsset: Codable {
    /// Faucet ID (issuer account ID)
    public let faucetId: String
    /// Asset amount
    public let amount: UInt64
    
    enum CodingKeys: String, CodingKey {
        case faucetId = "faucet_id"
        case amount
    }
}

/// Account balance information
public struct AccountBalance: Codable {
    /// Account ID
    public let accountId: String
    /// List of fungible assets
    public let fungibleAssets: [FungibleAsset]
    /// Total count of fungible assets
    public let totalFungibleCount: Int
    /// Total count of non-fungible assets
    public let totalNonFungibleCount: Int
    
    enum CodingKeys: String, CodingKey {
        case accountId = "account_id"
        case fungibleAssets = "fungible_assets"
        case totalFungibleCount = "total_fungible_count"
        case totalNonFungibleCount = "total_non_fungible_count"
    }
    
    /// Check if account has any assets
    public var hasAssets: Bool {
        totalFungibleCount > 0 || totalNonFungibleCount > 0
    }
    
    /// Get balance for specific faucet
    public func balance(for faucetId: String) -> UInt64 {
        fungibleAssets.first { $0.faucetId == faucetId }?.amount ?? 0
    }
}

/// Input Note information
public struct InputNoteInfo: Codable {
    /// Note ID
    public let noteId: String
    /// List of assets
    public let assets: [FungibleAsset]
    /// Whether authenticated
    public let isAuthenticated: Bool
    
    enum CodingKeys: String, CodingKey {
        case noteId = "note_id"
        case assets
        case isAuthenticated = "is_authenticated"
    }
    
    /// Get total asset value in Note (aggregated by faucet)
    public var totalAssets: [String: UInt64] {
        var totals: [String: UInt64] = [:]
        for asset in assets {
            totals[asset.faucetId, default: 0] += asset.amount
        }
        return totals
    }
}

/// Input Notes query result
public struct InputNotesResult: Codable {
    /// List of notes
    public let notes: [InputNoteInfo]
    /// Total count
    public let totalCount: Int
    
    enum CodingKeys: String, CodingKey {
        case notes
        case totalCount = "total_count"
    }
    
    /// Get all consumable note IDs
    public var consumableNoteIds: [String] {
        notes.map { $0.noteId }
    }
    
    /// Whether there are consumable notes
    public var hasConsumableNotes: Bool {
        !notes.isEmpty
    }
}