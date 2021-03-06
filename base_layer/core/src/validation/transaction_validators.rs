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

use crate::{
    chain_storage::{BlockchainBackend, BlockchainDatabase},
    transactions::{transaction::Transaction, types::CryptoFactories},
    validation::{Validation, ValidationError},
};
use std::sync::Arc;
use tari_utilities::hash::Hashable;

/// This validator will only check that a transaction is internally consistent. It requires no state information.
pub struct StatelessTxValidator {
    factories: Arc<CryptoFactories>,
}

impl StatelessTxValidator {
    pub fn new(factories: Arc<CryptoFactories>) -> Self {
        Self { factories }
    }
}

impl<B: BlockchainBackend> Validation<Transaction, B> for StatelessTxValidator {
    fn validate(&self, tx: &Transaction) -> Result<(), ValidationError> {
        verify_tx(tx, &self.factories)?;
        Ok(())
    }
}

/// This validator will perform a full verification of the transaction. In order the following will be checked:
/// Transaction integrity, All inputs exist in the backend, All timelocks (kernel lock heights and output maturities)
/// have passed
pub struct FullTxValidator<B: BlockchainBackend> {
    factories: Arc<CryptoFactories>,
    db: BlockchainDatabase<B>,
}

impl<B: BlockchainBackend> FullTxValidator<B>
where B: BlockchainBackend
{
    pub fn new(factories: Arc<CryptoFactories>, db: BlockchainDatabase<B>) -> Self {
        Self { factories, db }
    }
}

impl<B: BlockchainBackend> Validation<Transaction, B> for FullTxValidator<B> {
    fn validate(&self, tx: &Transaction) -> Result<(), ValidationError> {
        verify_tx(tx, &self.factories)?;
        verify_inputs(tx, self.db.clone())?;
        let height = self
            .db
            .get_height()
            .map_err(|e| ValidationError::CustomError(e.to_string()))?
            .unwrap_or(0);
        verify_timelocks(tx, height)?;
        Ok(())
    }
}

/// This validator assumes that the transaction was already validated and it will skip this step. It will only check, in
/// order,: All inputs exist in the backend, All timelocks (kernel lock heights and output maturities) have passed
pub struct TxInputAndMaturityValidator<B: BlockchainBackend> {
    db: BlockchainDatabase<B>,
}

impl<B: BlockchainBackend> TxInputAndMaturityValidator<B>
where B: BlockchainBackend
{
    pub fn new(db: BlockchainDatabase<B>) -> Self {
        Self { db }
    }
}

impl<B: BlockchainBackend> Validation<Transaction, B> for TxInputAndMaturityValidator<B> {
    fn validate(&self, tx: &Transaction) -> Result<(), ValidationError> {
        verify_inputs(tx, self.db.clone())?;
        let height = self
            .db
            .get_height()
            .map_err(|e| ValidationError::CustomError(e.to_string()))?
            .unwrap_or(0);
        verify_timelocks(tx, height)?;
        Ok(())
    }
}

/// This validator will only check that inputs exists in the backend.
pub struct InputTxValidator<B: BlockchainBackend> {
    db: BlockchainDatabase<B>,
}

impl<B: BlockchainBackend> InputTxValidator<B>
where B: BlockchainBackend
{
    pub fn new(db: BlockchainDatabase<B>) -> Self {
        Self { db }
    }
}

impl<B: BlockchainBackend> Validation<Transaction, B> for InputTxValidator<B> {
    fn validate(&self, tx: &Transaction) -> Result<(), ValidationError> {
        verify_inputs(tx, self.db.clone())?;
        Ok(())
    }
}

/// This validator will only check timelocks, it will check that kernel lock heights and output maturities have passed.
pub struct TimeLockTxValidator<B: BlockchainBackend> {
    db: BlockchainDatabase<B>,
}

impl<B: BlockchainBackend> TimeLockTxValidator<B>
where B: BlockchainBackend
{
    pub fn new(db: BlockchainDatabase<B>) -> Self {
        Self { db }
    }
}

impl<B: BlockchainBackend> Validation<Transaction, B> for TimeLockTxValidator<B> {
    fn validate(&self, tx: &Transaction) -> Result<(), ValidationError> {
        let height = self
            .db
            .get_height()
            .map_err(|e| ValidationError::CustomError(e.to_string()))?
            .unwrap_or(0);
        verify_timelocks(tx, height)?;
        Ok(())
    }
}

// This function verifies that the provided transaction is internally sound and that no funds were created in the
// transaction.
fn verify_tx(tx: &Transaction, factories: &CryptoFactories) -> Result<(), ValidationError> {
    tx.validate_internal_consistency(factories, None)
        .map_err(|e| ValidationError::TransactionError(e))
}

// This function checks that all the timelocks in the provided transaction pass. It checks kernel lock heights and
// input maturities
fn verify_timelocks(tx: &Transaction, current_height: u64) -> Result<(), ValidationError> {
    if tx.min_spendable_height() > current_height + 1 {
        return Err(ValidationError::MaturityError);
    }
    Ok(())
}

// This function checks that all inputs exist in the provided database backend
fn verify_inputs<B: BlockchainBackend>(tx: &Transaction, db: BlockchainDatabase<B>) -> Result<(), ValidationError> {
    for input in tx.body.inputs() {
        if !(db.is_utxo(input.hash())).map_err(|e| ValidationError::CustomError(e.to_string()))? {
            return Err(ValidationError::UnknownInputs);
        }
    }
    Ok(())
}
