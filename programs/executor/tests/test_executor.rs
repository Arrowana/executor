use std::collections::BTreeMap;

use anchor_lang::{
    prelude::*,
    solana_program::{
        message::Message, native_token::sol_to_lamports, nonce,
        system_instruction::create_nonce_account,
    },
    InstructionData,
};
use executor::{
    vault_transaction::{MultisigCompiledInstruction, MultisigMessageAddressTableLookup},
    VaultTransactionMessage,
};
use litesvm::LiteSVM;
use solana_address_lookup_table_interface::instruction::create_lookup_table;
use solana_sdk::{
    clock::Clock,
    instruction::Instruction,
    message::{
        v0::{self, LoadedAddresses, MessageAddressTableLookup},
        AccountKeys, AddressLookupTableAccount, CompileError, MessageHeader, VersionedMessage,
    },
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
    let tx = Transaction::new(
        &[&user_keypair, &nonce_keypair],
        Message::new(&ixs, Some(&user)),
        svm.latest_blockhash(),
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
    let advance_nonce = advance_nonce_account(&nonce_keypair.pubkey(), &user);
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
        addresses: vec![
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        ],
    };
    // Add relevant and blank accounts
    execute_ix.accounts.extend([
        AccountMeta::new(user, true), // Signers can't be in ALTs, so the only thing that isn't flexible is signer
        AccountMeta::new(address_lookup_table.addresses[0], false),
        AccountMeta::new(address_lookup_table.addresses[1], false),
        AccountMeta::new(address_lookup_table.addresses[2], false),
    ]);
    let address_lookup_table_accounts = &[address_lookup_table];

    let nonce_account = svm.get_account(&nonce_keypair.pubkey()).unwrap();
    let nonce::state::Versions::Current(state) =
        bincode::deserialize::<nonce::state::Versions>(&nonce_account.data).unwrap()
    else {
        panic!("Nonce account is not initialized");
    };
    let nonce::State::Initialized(nonce_data) = state.as_ref() else {
        panic!("");
    };
    let blockhash = nonce_data.blockhash();
    let execute_tx = VersionedTransaction::try_new(
        VersionedMessage::V0(
            v0::Message::try_compile(
                &user,
                &[advance_nonce, execute_ix],
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
    let v0_message = v0::Message::try_compile(
        &user,
        &[system_instruction::transfer(
            &user,
            &keeper,
            sol_to_lamports(0.5),
        )],
        address_lookup_table_accounts,
        blockhash,
    )
    .unwrap();
    let vault_transaction_message = try_compile_vault_transaction_message(
        vault_key,
        instructions,
        address_lookup_table_accounts,
    );
    let mut initialize_transaction_ix = Instruction {
        program_id: executor::ID,
        accounts: executor::accounts::InitializeTransaction {
            payer: keeper,
            transaction: transaction_keypair.pubkey(),
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: executor::instruction::InitializeTransaction {
            space: 10_000,
            vault_transaction_message,
        }
        .data(),
    };
    let tx = VersionedTransaction::try_new(
        VersionedMessage::V0(
            v0::Message::try_compile(
                &user,
                &[initialize_transaction_ix],
                address_lookup_table_accounts,
                blockhash,
            )
            .unwrap(),
        ),
        &[&user_keypair],
    )
    .unwrap();

    // Execute
    let tx_res = svm.send_transaction(execute_tx).unwrap();
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Invalid AddressLookupTableAccount")]
    InvalidAddressLookupTableAccount,
    #[error("Invalid TransactionMessage")]
    InvalidTransactionMessage,
}

/// This implementation is mostly a copy-paste from `solana_program::message::v0::Message::try_compile()`,
/// but it constructs a `TransactionMessage` meant to be passed to `vault_transaction_create`.
fn try_compile_vault_transaction_message(
    vault_key: &Pubkey,
    instructions: &[Instruction],
    address_lookup_table_accounts: &[AddressLookupTableAccount],
) -> std::result::Result<VaultTransactionMessage, CompileError> {
    let mut compiled_keys = CompiledKeys::compile(instructions, Some(*vault_key));

    let mut address_table_lookups = Vec::with_capacity(address_lookup_table_accounts.len());
    let mut loaded_addresses_list = Vec::with_capacity(address_lookup_table_accounts.len());
    for lookup_table_account in address_lookup_table_accounts {
        if let Some((lookup, loaded_addresses)) =
            compiled_keys.try_extract_table_lookup(lookup_table_account)?
        {
            address_table_lookups.push(lookup);
            loaded_addresses_list.push(loaded_addresses);
        }
    }

    let (header, static_keys) = compiled_keys.try_into_message_components()?;
    let dynamic_keys = loaded_addresses_list.into_iter().collect();
    let account_keys = AccountKeys::new(&static_keys, Some(&dynamic_keys));
    let instructions = account_keys.try_compile_instructions(instructions)?;

    let num_static_keys: u8 = static_keys
        .len()
        .try_into()
        .map_err(|_| CompileError::AccountIndexOverflow)?;

    Ok(VaultTransactionMessage {
        num_signers: header.num_required_signatures,
        num_writable_signers: header.num_required_signatures - header.num_readonly_signed_accounts,
        num_writable_non_signers: num_static_keys
            - header.num_required_signatures
            - header.num_readonly_unsigned_accounts,
        account_keys: static_keys.into(),
        instructions: instructions
            .into_iter()
            .map(|ix| MultisigCompiledInstruction {
                program_id_index: ix.program_id_index,
                account_indexes: ix.accounts.into(),
                data: ix.data.into(),
            })
            .collect::<Vec<_>>()
            .into(),
        address_table_lookups: address_table_lookups
            .into_iter()
            .map(|lookup| MultisigMessageAddressTableLookup {
                account_key: lookup.account_key,
                writable_indexes: lookup.writable_indexes.into(),
                readonly_indexes: lookup.readonly_indexes.into(),
            })
            .collect::<Vec<_>>()
            .into(),
    })
}

/// A helper struct to collect pubkeys compiled for a set of instructions
///
/// NOTE: The only difference between this and the original implementation from `solana_program` is that we don't mark the instruction programIds as invoked.
// /// It makes sense to do because the instructions will be called via CPI, so the programIds can come from Address Lookup Tables.
// /// This allows to compress the message size and avoid hitting the tx size limit during `vault_transaction_create` instruction calls.
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompiledKeys {
    payer: Option<Pubkey>,
    key_meta_map: BTreeMap<Pubkey, CompiledKeyMeta>,
}

#[derive(Default, Debug, Clone, PartialEq, Eq)]
struct CompiledKeyMeta {
    is_signer: bool,
    is_writable: bool,
    is_invoked: bool,
}

impl CompiledKeys {
    /// Compiles the pubkeys referenced by a list of instructions and organizes by
    /// signer/non-signer and writable/readonly.
    pub(crate) fn compile(instructions: &[Instruction], payer: Option<Pubkey>) -> Self {
        let mut key_meta_map = BTreeMap::<Pubkey, CompiledKeyMeta>::new();
        for ix in instructions {
            let meta = key_meta_map.entry(ix.program_id).or_default();
            // NOTE: This is the only difference from the original.
            // meta.is_invoked = true;
            meta.is_invoked = false;
            for account_meta in &ix.accounts {
                let meta = key_meta_map.entry(account_meta.pubkey).or_default();
                meta.is_signer |= account_meta.is_signer;
                meta.is_writable |= account_meta.is_writable;
            }
        }
        if let Some(payer) = &payer {
            let meta = key_meta_map.entry(*payer).or_default();
            meta.is_signer = true;
            meta.is_writable = true;
        }
        Self {
            payer,
            key_meta_map,
        }
    }

    pub(crate) fn try_into_message_components(
        self,
    ) -> Result<(MessageHeader, Vec<Pubkey>), CompileError> {
        let try_into_u8 = |num: usize| -> Result<u8, CompileError> {
            u8::try_from(num).map_err(|_| CompileError::AccountIndexOverflow)
        };

        let Self {
            payer,
            mut key_meta_map,
        } = self;

        if let Some(payer) = &payer {
            key_meta_map.remove_entry(payer);
        }

        let writable_signer_keys: Vec<Pubkey> = payer
            .into_iter()
            .chain(
                key_meta_map
                    .iter()
                    .filter_map(|(key, meta)| (meta.is_signer && meta.is_writable).then_some(*key)),
            )
            .collect();
        let readonly_signer_keys: Vec<Pubkey> = key_meta_map
            .iter()
            .filter_map(|(key, meta)| (meta.is_signer && !meta.is_writable).then_some(*key))
            .collect();
        let writable_non_signer_keys: Vec<Pubkey> = key_meta_map
            .iter()
            .filter_map(|(key, meta)| (!meta.is_signer && meta.is_writable).then_some(*key))
            .collect();
        let readonly_non_signer_keys: Vec<Pubkey> = key_meta_map
            .iter()
            .filter_map(|(key, meta)| (!meta.is_signer && !meta.is_writable).then_some(*key))
            .collect();

        let signers_len = writable_signer_keys
            .len()
            .saturating_add(readonly_signer_keys.len());

        let header = MessageHeader {
            num_required_signatures: try_into_u8(signers_len)?,
            num_readonly_signed_accounts: try_into_u8(readonly_signer_keys.len())?,
            num_readonly_unsigned_accounts: try_into_u8(readonly_non_signer_keys.len())?,
        };

        let static_account_keys = std::iter::empty()
            .chain(writable_signer_keys)
            .chain(readonly_signer_keys)
            .chain(writable_non_signer_keys)
            .chain(readonly_non_signer_keys)
            .collect();

        Ok((header, static_account_keys))
    }

    #[cfg(not(target_os = "solana"))]
    pub(crate) fn try_extract_table_lookup(
        &mut self,
        lookup_table_account: &AddressLookupTableAccount,
    ) -> Result<Option<(MessageAddressTableLookup, LoadedAddresses)>, CompileError> {
        let (writable_indexes, drained_writable_keys) = self
            .try_drain_keys_found_in_lookup_table(&lookup_table_account.addresses, |meta| {
                !meta.is_signer && !meta.is_invoked && meta.is_writable
            })?;
        let (readonly_indexes, drained_readonly_keys) = self
            .try_drain_keys_found_in_lookup_table(&lookup_table_account.addresses, |meta| {
                !meta.is_signer && !meta.is_invoked && !meta.is_writable
            })?;

        // Don't extract lookup if no keys were found
        if writable_indexes.is_empty() && readonly_indexes.is_empty() {
            return Ok(None);
        }

        Ok(Some((
            MessageAddressTableLookup {
                account_key: lookup_table_account.key,
                writable_indexes,
                readonly_indexes,
            },
            LoadedAddresses {
                writable: drained_writable_keys,
                readonly: drained_readonly_keys,
            },
        )))
    }

    #[cfg(not(target_os = "solana"))]
    fn try_drain_keys_found_in_lookup_table(
        &mut self,
        lookup_table_addresses: &[Pubkey],
        key_meta_filter: impl Fn(&CompiledKeyMeta) -> bool,
    ) -> Result<(Vec<u8>, Vec<Pubkey>), CompileError> {
        let mut lookup_table_indexes = Vec::new();
        let mut drained_keys = Vec::new();

        for search_key in self
            .key_meta_map
            .iter()
            .filter_map(|(key, meta)| key_meta_filter(meta).then_some(key))
        {
            for (key_index, key) in lookup_table_addresses.iter().enumerate() {
                if key == search_key {
                    let lookup_table_index = u8::try_from(key_index)
                        .map_err(|_| CompileError::AddressTableLookupIndexOverflow)?;

                    lookup_table_indexes.push(lookup_table_index);
                    drained_keys.push(*search_key);
                    break;
                }
            }
        }

        for key in &drained_keys {
            self.key_meta_map.remove_entry(key);
        }

        Ok((lookup_table_indexes, drained_keys))
    }
}
