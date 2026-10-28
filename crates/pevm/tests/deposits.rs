//! Test RISE blocks with deposit transactions, which have no nonce check, pay no fees,
//! and mint ETH to their sender before execution.

use std::collections::HashMap;

use common::MockEvmTx;
use op_revm::{
    OpTransaction,
    transaction::deposit::{DEPOSIT_TRANSACTION_TYPE, DepositTransactionParts},
};
use pevm::{InMemoryStorage, chain::PevmRise};
use revm::{
    context::{TransactTo, TxEnv},
    primitives::{Address, B256, U256, alloy_primitives::U160},
};

pub mod common;

const BLOCK_SIZE: usize = 10_000;

// Mock user addresses after the precompiles, so transfers stay plain value transfers.
fn mock_address(idx: usize) -> Address {
    Address::from(U160::from(0x1000 + idx))
}

fn mock_deposit(caller: Address, to: Address, value: u64, mint: u128) -> OpTransaction<TxEnv> {
    OpTransaction {
        base: TxEnv {
            tx_type: DEPOSIT_TRANSACTION_TYPE,
            // Deposits have no chain ID nor nonce, and pay no gas.
            chain_id: None,
            caller,
            kind: TransactTo::Call(to),
            value: U256::from(value),
            gas_limit: common::RAW_TRANSFER_GAS_LIMIT,
            ..TxEnv::default()
        },
        enveloped_tx: None,
        deposit: DepositTransactionParts::new(B256::ZERO, Some(mint), false),
    }
}

fn mock_transfer(caller: Address, nonce: u64, to: Address) -> OpTransaction<TxEnv> {
    PevmRise.mock_evm_tx(TxEnv {
        caller,
        nonce,
        kind: TransactTo::Call(to),
        value: U256::from(1),
        gas_limit: common::RAW_TRANSFER_GAS_LIMIT,
        gas_price: 1,
        ..TxEnv::default()
    })
}

// Deposits from fresh accounts that only exist through their minted ETH.
#[test]
fn rise_deposits_independent() {
    common::test_execute_revm(
        &PevmRise,
        InMemoryStorage::default(),
        (0..BLOCK_SIZE)
            .map(|i| {
                let address = mock_address(i);
                mock_deposit(address, address, 1, 1_000)
            })
            .collect(),
    );
}

// The same depositor repeatedly minting and sending ETH to the same recipient.
#[test]
fn rise_deposits_same_sender() {
    let depositor = mock_address(0);
    let recipient = mock_address(1);
    common::test_execute_revm(
        &PevmRise,
        InMemoryStorage::default(),
        (0..BLOCK_SIZE)
            .map(|_| mock_deposit(depositor, recipient, 1, 1_000))
            .collect(),
    );
}

// Deposits fund fresh accounts that then spend the minted ETH later in the block.
#[test]
fn rise_deposits_then_spend() {
    let addresses: Vec<_> = (0..BLOCK_SIZE / 2).map(mock_address).collect();
    common::test_execute_revm(
        &PevmRise,
        // Mock the beneficiary account (`Address:ZERO`).
        InMemoryStorage::new(
            [common::mock_account(0)].into_iter().collect(),
            Default::default(),
            Default::default(),
        ),
        addresses
            .iter()
            .map(|&address| mock_deposit(address, address, 0, 1_000_000))
            // The deposits above bumped the nonce of each account to 1.
            .chain(
                addresses
                    .iter()
                    .map(|&address| mock_transfer(address, 1, Address::ZERO)),
            )
            .collect(),
    );
}

// Deposits interleaved with regular transfers from the same senders. Deposits skip
// the nonce check but still bump the sender's nonce for the following transfers.
#[test]
fn rise_deposits_interleaved_with_transfers() {
    let num_senders = 10;
    let mut nonces = HashMap::new();
    common::test_execute_revm(
        &PevmRise,
        // Mock the beneficiary account (`Address:ZERO`) and the senders.
        InMemoryStorage::new(
            [common::mock_account(0)]
                .into_iter()
                .chain((0..num_senders).map(|i| {
                    let (_, account) = common::mock_account(i);
                    (mock_address(i), account)
                }))
                .collect(),
            Default::default(),
            Default::default(),
        ),
        (0..BLOCK_SIZE)
            .map(|i| {
                let address = mock_address(i % num_senders);
                // [common::mock_account] starts at nonce 1.
                let nonce = nonces.entry(address).or_insert(1);
                let tx = if i % 3 == 0 {
                    mock_deposit(address, address, 1, 1_000)
                } else {
                    mock_transfer(address, *nonce, address)
                };
                *nonce += 1;
                tx
            })
            .collect(),
    );
}
