use anchor_lang::{
    prelude::*,
    solana_program::{
        message::Message, native_token::sol_to_lamports, nonce,
        system_instruction::create_nonce_account,
    },
    InstructionData,
};

use litesvm::LiteSVM;
use solana_address_lookup_table_interface::instruction::{
    create_lookup_table, extend_lookup_table,
};
use solana_sdk::{
    clock::Clock,
    instruction::Instruction,
    message::{
        v0::{self},
        AddressLookupTableAccount, VersionedMessage,
    },
    nonce::state::DurableNonce,
    pubkey::Pubkey,
    signature::Keypair,
    signer::Signer,
    system_instruction::{self, advance_nonce_account},
    system_program,
    transaction::{Transaction, VersionedTransaction},
};

#[test]
fn test_executor() {
    let keeper_keypair = Keypair::new();
    let keeper = keeper_keypair.pubkey();
    let user_keypair = Keypair::new();
    let user = user_keypair.pubkey();

    let mut svm = LiteSVM::new();
    svm.add_program_from_file(executor::ID, "../../target/deploy/executor.so")
        .unwrap();
    svm.airdrop(&keeper_keypair.pubkey(), sol_to_lamports(1.))
        .unwrap();
    svm.airdrop(&user, sol_to_lamports(1.)).unwrap();

    let nonce_keypair = Keypair::new();
    let nonce_rent = svm.minimum_balance_for_rent_exemption(nonce::State::size());
    let ixs = create_nonce_account(
        &user,
        &nonce_keypair.pubkey(),
        &user,
        nonce_rent + 200_000_000,
    );
    let create_nonce_blockhash = svm.latest_blockhash();
    let tx = Transaction::new(
        &[&user_keypair, &nonce_keypair],
        Message::new(&ixs, Some(&user)),
        create_nonce_blockhash,
    );
    let tx_res = svm.send_transaction(tx).unwrap();

    // Setup an ALT
    let recent_slot = svm.get_sysvar::<Clock>().slot;
    let (ix, lookup_table_address) = create_lookup_table(keeper, keeper, recent_slot);
    let tx = Transaction::new(
        &[&keeper_keypair],
        Message::new(&[ix], Some(&keeper)),
        svm.latest_blockhash(),
    );
    let tx_res = svm.send_transaction(tx).unwrap();

    // Sign a blank execute instruction, pointing
    let advance_nonce_ix = advance_nonce_account(&nonce_keypair.pubkey(), &user);
    let transaction_keypair = Keypair::new();
    let mut execute_ix = Instruction {
        program_id: executor::ID,
        accounts: executor::accounts::Execute {
            transaction: transaction_keypair.pubkey(),
        }
        .to_account_metas(None),
        data: executor::instruction::Execute {}.data(),
    };
    let address_lookup_table = AddressLookupTableAccount {
        key: lookup_table_address,
        addresses: vec![Pubkey::new_unique(), Pubkey::new_unique()],
    };
    // Add relevant and blank accounts
    execute_ix.accounts.extend([
        AccountMeta::new(user, true), // Signers can't be in ALTs, so the only thing that isn't flexible is signer
        AccountMeta::new(address_lookup_table.addresses[0], false),
        AccountMeta::new(address_lookup_table.addresses[1], false),
    ]);
    let address_lookup_table_accounts = &[address_lookup_table];

    let nonce_account = svm.get_account(&nonce_keypair.pubkey()).unwrap();
    let nonce::state::Versions::Current(state) =
        bincode::deserialize::<nonce::state::Versions>(&nonce_account.data).unwrap()
    else {
        panic!("Nonce version cannot be deserialized");
    };
    let nonce::State::Initialized(nonce_data) = state.as_ref() else {
        panic!("Nonce is not initialized");
    };
    let blockhash = nonce_data.blockhash();
    let execute_tx = VersionedTransaction::try_new(
        VersionedMessage::V0(
            v0::Message::try_compile(
                &user,
                &[advance_nonce_ix, execute_ix],
                address_lookup_table_accounts,
                blockhash,
            )
            .unwrap(),
        ),
        &[&user_keypair],
    )
    .unwrap();

    // Wait...
    svm.warp_to_slot(svm.get_sysvar::<Clock>().slot + 1_000);

    // Executor knows what to do, prepares for execution
    // We do a simple transfer of sol to x
    let transfer_ix = system_instruction::transfer(&user, &keeper, sol_to_lamports(0.5));
    let transaction = executor::Transaction {
        // Manually compiled
        compiled_instructions: vec![executor::CompiledInstruction {
            program_id_index: 1,
            accounts: vec![0, 2],
            data: transfer_ix.data,
        }],
    };
    let initialize_transaction_ix = Instruction {
        program_id: executor::ID,
        accounts: executor::accounts::InitializeTransaction {
            payer: keeper,
            transaction: transaction_keypair.pubkey(),
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: executor::instruction::InitializeTransaction {
            space: 10_000,
            transaction,
        }
        .data(),
    };
    let extend_lookup_table_ix = extend_lookup_table(
        lookup_table_address,
        keeper,
        Some(keeper),
        vec![system_program::ID, keeper],
    );
    let tx = Transaction::new(
        &[&keeper_keypair, &transaction_keypair],
        Message::new(
            &[initialize_transaction_ix, extend_lookup_table_ix],
            Some(&keeper),
        ),
        svm.latest_blockhash(),
    );
    let tx_res = svm.send_transaction(tx).unwrap();

    // Go over the slot so the ALT is updated
    svm.warp_to_slot(svm.get_sysvar::<Clock>().slot + 1);
    svm.expire_blockhash(); // Otherwise durable nonce cannot work

    // Execute
    let tx_res = svm.send_transaction(execute_tx).unwrap();
    println!("Transaction result logs: {:#?}", tx_res.logs);
}
