# Executor

A keeper executes an arbitrary set of instructions on behalf of the user, given a blank transaction signed by the user

Steps:

- The signs a "blank" transaction with a durable nonce calling execute on a given transaction account, the transaction uses addresses from a lookup table which are not populated
- The keeper then initializes the referenced transaction account, and extends the address lookup table with the necessary addresses
- The keeper then sends the original user transaction

[test_executor full functional example](programs/executor/tests/test_executor.rs)

Note:

- This could be made more compressed and fancy using squads v4 vault transaction message
- In this example there is no validation it gives ultimate power to the keeper, the only limitation is the number of blank addresses it signed for. A better approach for a specific use case would be to constrain and validate the action
