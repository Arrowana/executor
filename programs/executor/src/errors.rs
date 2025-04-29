use anchor_lang::error_code;

#[error_code]
pub enum ExecutorError {
    InvalidNumberOfAccounts,
    InvalidAccount,
    InvalidTransactionMessage,
}
