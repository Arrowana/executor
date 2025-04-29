use anchor_lang::{
    prelude::*,
    solana_program::{instruction::Instruction, program::invoke},
};
mod errors;

declare_id!("G69kA5xoXCavcekguee2Md15nw7Gd6Q44EjaAcJNx1yi");

#[program]
pub mod executor {
    use super::*;

    pub fn initialize_transaction(
        ctx: Context<InitializeTransaction>,
        space: usize,
        transaction: Transaction,
    ) -> Result<()> {
        msg!("Transaction initialized");
        ctx.accounts.transaction.set_inner(transaction);
        Ok(())
    }

    pub fn execute(ctx: Context<Execute>) -> Result<()> {
        let compiled_instructions =
            std::mem::take(&mut ctx.accounts.transaction.compiled_instructions);
        let remaining_accounts = ctx.remaining_accounts;
        for compiled_instruction in compiled_instructions {
            let program_ai = remaining_accounts
                .get(usize::from(compiled_instruction.program_id_index))
                .unwrap();

            let mut accounts = Vec::new();
            let mut account_infos = Vec::new();
            for account_index in compiled_instruction.accounts {
                let account_info = remaining_accounts.get(usize::from(account_index)).unwrap();
                accounts.push(AccountMeta {
                    pubkey: account_info.key(),
                    is_signer: account_info.is_signer,
                    is_writable: account_info.is_writable,
                });
                account_infos.push(account_info.clone());
            }

            invoke(
                &Instruction {
                    program_id: program_ai.key(),
                    accounts,
                    data: compiled_instruction.data,
                },
                &account_infos,
            )?;
        }

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

/// Simplified with no compression to make the POC straightforward, see squads v4 for how to compress
#[account]
pub struct Transaction {
    pub compiled_instructions: Vec<CompiledInstruction>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct CompiledInstruction {
    /// Index into the transaction keys array indicating the program account that executes this instruction.
    pub program_id_index: u8,
    /// Ordered indices into the transaction keys array indicating which accounts to pass to the program.
    pub accounts: Vec<u8>,
    /// The program input data.
    pub data: Vec<u8>,
}
