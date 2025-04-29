use anchor_lang::prelude::*;
mod errors;
mod executable_transaction_message;
mod small_vec;
pub mod vault_transaction;

use crate::{errors::ExecutorError, executable_transaction_message::ExecutableTransactionMessage};

pub use vault_transaction::VaultTransactionMessage;

declare_id!("G69kA5xoXCavcekguee2Md15nw7Gd6Q44EjaAcJNx1yi");

#[program]
pub mod executor {
    use super::*;

    pub fn initialize_transaction(
        ctx: Context<InitializeTransaction>,
        space: usize,
        vault_transaction_message: VaultTransactionMessage,
    ) -> Result<()> {
        msg!("Transaction initialized");
        ctx.accounts.transaction.vault_transaction_message = vault_transaction_message;
        Ok(())
    }

    pub fn execute(ctx: Context<Execute>) -> Result<()> {
        let transaction_message =
            core::mem::take(&mut ctx.accounts.transaction.vault_transaction_message);
        let num_lookups = transaction_message.address_table_lookups.len();

        let message_account_infos = ctx
            .remaining_accounts
            .get(num_lookups..)
            .ok_or(ExecutorError::InvalidNumberOfAccounts)?;
        let address_lookup_table_account_infos = ctx
            .remaining_accounts
            .get(..num_lookups)
            .ok_or(ExecutorError::InvalidNumberOfAccounts)?;

        let executable_message = ExecutableTransactionMessage::new_validated(
            transaction_message,
            message_account_infos,
            address_lookup_table_account_infos,
            // &vault_pubkey,
            // &ephemeral_signer_keys,
        )?;

        // Execute the transaction message instructions one-by-one.
        // NOTE: `execute_message()` calls `self.to_instructions_and_accounts()`
        // which in turn calls `take()` on
        // `self.message.instructions`, therefore after this point no more
        // references or usages of `self.message` should be made to avoid
        // faulty behavior.
        executable_message.execute_message()?; //vault_seeds, &ephemeral_signer_seeds, &[])?;

        Ok(())
    }
}

#[derive(Accounts)]
#[instruction(space: usize)]
pub struct InitializeTransaction<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(
        init,
        payer = payer,
        space = space,
    )]
    pub transaction: Account<'info, Transaction>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Execute<'info> {
    pub transaction: Account<'info, Transaction>,
}

#[account]
pub struct Transaction {
    pub vault_transaction_message: VaultTransactionMessage,
}
