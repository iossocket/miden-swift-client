import SwiftUI
import MidenSwiftClient

struct ContentView: View {
    @State private var wallet: MidenWallet?
    @State private var accounts: [String] = []
    @State private var errorMessage: String?

    var body: some View {
        VStack(spacing: 20) {
            if let error = errorMessage {
                Text("Error: \(error)")
                    .foregroundColor(.red)
                    .padding()
            }

            Button("Create new") {
                Task {
                    do {
                        guard let wallet = wallet else { return }
                        let newAccount = try await wallet.createWalletAsync()
                        print("New account: \(newAccount)")
                        await refreshAccounts()
                    } catch {
                        errorMessage = error.localizedDescription
                        print("Error creating wallet: \(error)")
                    }
                }
            }

            Button("Get Notes") {
                Task {
                    do {
                        guard let wallet = wallet else {
                            return
                        }
                        let res = try await wallet.syncAsync()
                        print("Synced to block: \(res)")
                        
                        let accounts = try await wallet.getAccountsAsync()
                        print("Accounts: \(accounts)")
                        
                        if accounts.count == 0 {
                            print("No accounts found")
                            return
                        }
                        let account = accounts[0]

                        let notes = try await wallet.getInputNotesAsync(accountId: account)
                        print("Found \(notes.totalCount) consumable notes")

                        for note in notes.notes {
                            print("Note: \(note.noteId)")
                            for asset in note.assets {
                                print("  - \(asset.amount) from \(asset.faucetId)")
                            }
                        }

                        // Sync again before consuming
                        _ = try await wallet.syncAsync()
                        
                        if !notes.consumableNoteIds.isEmpty {
                            let txId = try await wallet.consumeNotesAsync(
                                accountId: account,
                                noteIds: notes.consumableNoteIds
                            )
                            print("Transaction submitted: \(txId)")
                        }
                    } catch {
                        errorMessage = error.localizedDescription
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
                        _ = try await wallet.syncAsync()
                        
                        let accounts = try await wallet.getAccountsAsync()
                        if accounts.count == 0 {
                            print("No accounts found")
                            return
                        }
                        let account = accounts[0]
                        
                        let balance = try await wallet.getBalanceAsync(accountId: account)
                        print("Total fungible: \(balance.totalFungibleCount)")

                        for asset in balance.fungibleAssets {
                            print("Faucet: \(asset.faucetId), Amount: \(asset.amount)")
                        }
                    } catch {
                        errorMessage = error.localizedDescription
                        print("Error: \(error)")
                    }
                }
            }
        }
        .padding()
        .onAppear {
            do {
                wallet = try MidenWallet()
                print("MidenWallet created successfully")
                print("Keystore ready: \(wallet!.isKeystoreReady)")
                print("Store ready: \(wallet!.isStoreReady)")
                print("Keystore path: \(wallet!.keystoreDirectory)")
                print("Store path: \(wallet!.storeFile)")
                
                // Load accounts on appear
                Task {
                    await refreshAccounts()
                }
            } catch {
                errorMessage = "Failed to create MidenWallet: \(error)"
                print("Failed to create MidenWallet: \(error)")
            }
        }
    }

    private func refreshAccounts() async {
        do {
            guard let wallet = wallet else { return }
            accounts = try await wallet.getAccountsAsync()
            print("Refreshed accounts: \(accounts)")
        } catch {
            errorMessage = error.localizedDescription
            print("Error refreshing accounts: \(error)")
        }
    }
}