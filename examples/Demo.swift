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