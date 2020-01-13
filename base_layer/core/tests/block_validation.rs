// Copyright 2019. The Tari Project
//
// Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
// following conditions are met:
//
// 1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
// disclaimer.
//
// 2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
// following disclaimer in the documentation and/or other materials provided with the distribution.
//
// 3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
// products derived from this software without specific prior written permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
// INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
// DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
// SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
// SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
// WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
// USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

mod helpers;
use crate::helpers::block_builders::generate_new_block_with_coinbase;
use helpers::block_builders::{create_genesis_block_with_utxos, generate_new_block};
use std::sync::Arc;
use tari_core::{
    blocks::genesis_block::get_genesis_block,
    chain_storage::{BlockAddResult, BlockchainDatabase, MemoryDatabase, Validators},
    consensus::ConsensusManager,
    proof_of_work::DiffAdjManager,
    validation::{
        block_validators::{FullConsensusValidator, StatelessValidator},
        mocks::MockValidator,
    },
};
use tari_transactions::{
    tari_amount::{uT, MicroTari, T},
    txn_schema,
    types::{CryptoFactories, HashDigest},
};

#[test]
fn test_genesis_block() {
    let factories = Arc::new(CryptoFactories::default());
    let rules = ConsensusManager::default();
    let backend = MemoryDatabase::<HashDigest>::default();
    let mut db = BlockchainDatabase::new(backend).unwrap();
    let validators = Validators::new(
        FullConsensusValidator::new(rules.clone(), factories.clone(), db.clone()),
        StatelessValidator::new(factories.clone()),
        MockValidator::new(true),
    );
    db.set_validators(validators);
    let diff_adj_manager = DiffAdjManager::new(db.clone()).unwrap();
    rules.set_diff_manager(diff_adj_manager).unwrap();
    let block = get_genesis_block();
    let result = db.add_block(block);
    assert!(result.is_ok());
}

#[test]
fn test_valid_chain() {
    let factories = Arc::new(CryptoFactories::default());
    let rules = ConsensusManager::default();
    let backend = MemoryDatabase::<HashDigest>::default();
    let mut db = BlockchainDatabase::new(backend).unwrap();
    let validators_true = Validators::new(
        FullConsensusValidator::new(rules.clone(), factories.clone(), db.clone()),
        StatelessValidator::new(factories.clone()),
    );
    let validators_false = Validators::new(MockValidator::new(true), MockValidator::new(true));
    db.set_validators(validators_false);
    let diff_adj_manager = DiffAdjManager::new(db.clone()).unwrap();
    rules.set_diff_manager(diff_adj_manager).unwrap();

    let (block0, output) = create_genesis_block_with_utxos(&db, &factories, &[10 * T]);
    db.add_block(block0.clone()).unwrap();
    let mut blocks = vec![block0];
    let mut outputs = vec![output];
    db.set_validators(validators_true);
    // Block 1
    let schema = vec![txn_schema!(from: vec![outputs[0][1].clone()], to: vec![6 * T, 3 * T])];
    assert_eq!(
        generate_new_block_with_coinbase(&mut db, &mut blocks, &mut outputs, schema, rules.clone()),
        Ok(BlockAddResult::Ok)
    );
    // Block 2
    let schema = vec![txn_schema!(from: vec![outputs[1][0].clone()], to: vec![3 * T, 1 * T])];
    assert_eq!(
        generate_new_block_with_coinbase(&mut db, &mut blocks, &mut outputs, schema, rules.clone()),
        Ok(BlockAddResult::Ok)
    );
    // Block 3
    let schema = vec![
        txn_schema!(from: vec![outputs[2][0].clone()], to: vec![2 * T, 500_000 * uT]),
        txn_schema!(from: vec![outputs[1][1].clone()], to: vec![500_000 * uT]),
    ];
    assert_eq!(
        generate_new_block_with_coinbase(&mut db, &mut blocks, &mut outputs, schema, rules.clone()),
        Ok(BlockAddResult::Ok)
    );
}

#[test]
fn test_invalid_coinbase() {
    let factories = Arc::new(CryptoFactories::default());
    let rules = ConsensusManager::default();
    let backend = MemoryDatabase::<HashDigest>::default();
    let mut db = BlockchainDatabase::new(backend).unwrap();
    let validators_true = Validators::new(
        FullConsensusValidator::new(rules.clone(), factories.clone(), db.clone()),
        StatelessValidator::new(factories.clone()),
    );
    let validators_false = Validators::new(MockValidator::new(true), MockValidator::new(true));
    db.set_validators(validators_false);
    let diff_adj_manager = DiffAdjManager::new(db.clone()).unwrap();
    rules.set_diff_manager(diff_adj_manager).unwrap();

    let (block0, output) = create_genesis_block_with_utxos(&db, &factories, &[10 * T]);
    db.add_block(block0.clone()).unwrap();
    let mut blocks = vec![block0];
    let mut outputs = vec![output];
    db.set_validators(validators_true);
    let schema = vec![txn_schema!(from: vec![outputs[0][1].clone()], to: vec![6 * T, 3 * T])];
    // We have no coinbase, so this should fail
    assert!(generate_new_block(&mut db, &mut blocks, &mut outputs, schema).is_err());
}
