// Copyright 2018 The Tari Project
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
//
// Portions of this file were originally copyrighted (c) 2018 The Grin Developers, issued under the Apache License,
// Version 2.0, available at http://www.apache.org/licenses/LICENSE-2.0.
use crate::{
    blocks::BlockHeader,
    consensus::ConsensusConstants,
    proof_of_work::ProofOfWork,
    transactions::{
        aggregated_body::AggregateBody,
        tari_amount::MicroTari,
        transaction::{
            OutputFlags,
            Transaction,
            TransactionError,
            TransactionInput,
            TransactionKernel,
            TransactionOutput,
        },
    },
};
use derive_error::Error;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use tari_utilities::Hashable;

#[derive(Clone, Debug, PartialEq, Error)]
pub enum BlockValidationError {
    // A transaction in the block failed to validate
    TransactionError(TransactionError),
    // Invalid kernel in block
    InvalidKernel,
    // Invalid input in block
    InvalidInput,
    // Input maturity not reached
    InputMaturity,
    // Invalid coinbase maturity in block or more than one coinbase
    InvalidCoinbase,
    // Mismatched MMR roots
    MismatchedMmrRoots,
}

/// A Tari block. Blocks are linked together into a blockchain.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Block {
    pub header: BlockHeader,
    pub body: AggregateBody,
}

impl Block {
    /// This function will calculate the total fees contained in a block
    pub fn calculate_fees(&self) -> MicroTari {
        self.body.kernels().iter().fold(0.into(), |sum, x| sum + x.fee)
    }

    /// This function will check spent kernel rules like tx lock height etc
    pub fn check_kernel_rules(&self) -> Result<(), BlockValidationError> {
        for kernel in self.body.kernels() {
            if kernel.lock_height > self.header.height {
                return Err(BlockValidationError::InvalidKernel);
            }
        }
        Ok(())
    }

    /// Run through the outputs of the block and check that
    /// 1. There is exactly ONE coinbase output
    /// 1. The output's maturity is correctly set
    /// NOTE this does not check the coinbase amount
    pub fn check_coinbase_output(&self) -> Result<(), BlockValidationError> {
        let mut coinbase_counter = 0; // there should be exactly 1 coinbase
        for utxo in self.body.outputs() {
            if utxo.features.flags.contains(OutputFlags::COINBASE_OUTPUT) {
                coinbase_counter += 1;
                if utxo.features.maturity < (self.header.height + ConsensusConstants::current().coinbase_lock_height())
                {
                    return Err(BlockValidationError::InvalidCoinbase);
                }
            }
        }
        if coinbase_counter != 1 {
            return Err(BlockValidationError::InvalidCoinbase);
        }
        Ok(())
    }

    /// This function will check all stxo to ensure that feature flags where followed
    pub fn check_stxo_rules(&self) -> Result<(), BlockValidationError> {
        for input in self.body.inputs() {
            if input.features.maturity > self.header.height {
                return Err(BlockValidationError::InputMaturity);
            }
        }
        Ok(())
    }

    /// Destroys the block and returns the pieces of the block: header, inputs, outputs and kernels
    pub fn dissolve(
        self,
    ) -> (
        BlockHeader,
        Vec<TransactionInput>,
        Vec<TransactionOutput>,
        Vec<TransactionKernel>,
    ) {
        let (i, o, k) = self.body.dissolve();
        (self.header, i, o, k)
    }
}

impl Display for Block {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        fmt.write_str("----------------- Block -----------------\n")?;
        fmt.write_str("--- Header ---\n")?;
        fmt.write_str(&format!("{}\n", self.header))?;
        fmt.write_str("---  Body  ---\n")?;
        fmt.write_str(&format!("{}\n", self.body))
    }
}

pub struct BlockBuilder {
    header: BlockHeader,
    inputs: Vec<TransactionInput>,
    outputs: Vec<TransactionOutput>,
    kernels: Vec<TransactionKernel>,
    total_fee: MicroTari,
}

impl BlockBuilder {
    pub fn new() -> BlockBuilder {
        BlockBuilder {
            header: BlockHeader::new(ConsensusConstants::current().blockchain_version()),
            inputs: Vec::new(),
            outputs: Vec::new(),
            kernels: Vec::new(),
            total_fee: MicroTari::from(0),
        }
    }

    /// This function adds a header to the block
    pub fn with_header(mut self, header: BlockHeader) -> Self {
        self.header = header;
        self
    }

    /// This function adds the provided transaction inputs to the block
    pub fn add_inputs(mut self, mut inputs: Vec<TransactionInput>) -> Self {
        self.inputs.append(&mut inputs);
        self
    }

    /// This function adds the provided transaction outputs to the block
    pub fn add_outputs(mut self, mut outputs: Vec<TransactionOutput>) -> Self {
        self.outputs.append(&mut outputs);
        self
    }

    /// This function adds the provided transaction kernels to the block
    pub fn add_kernels(mut self, mut kernels: Vec<TransactionKernel>) -> Self {
        for kernel in &kernels {
            self.total_fee += kernel.fee;
        }
        self.kernels.append(&mut kernels);
        self
    }

    /// This functions add the provided transactions to the block
    pub fn with_transactions(mut self, txs: Vec<Transaction>) -> Self {
        let iter = txs.into_iter();
        for tx in iter {
            let (inputs, outputs, kernels) = tx.body.dissolve();
            self = self.add_inputs(inputs);
            self = self.add_outputs(outputs);
            self = self.add_kernels(kernels);
            self.header.total_kernel_offset = self.header.total_kernel_offset + tx.offset;
        }
        self
    }

    /// This functions add the provided transactions to the block
    pub fn add_transaction(mut self, tx: Transaction) -> Self {
        let (inputs, outputs, kernels) = tx.body.dissolve();
        self = self.add_inputs(inputs);
        self = self.add_outputs(outputs);
        self = self.add_kernels(kernels);
        self.header.total_kernel_offset = &self.header.total_kernel_offset + &tx.offset;
        self
    }

    /// This will add the given coinbase UTXO to the block
    pub fn with_coinbase_utxo(mut self, coinbase_utxo: TransactionOutput, coinbase_kernel: TransactionKernel) -> Self {
        self.kernels.push(coinbase_kernel);
        self.outputs.push(coinbase_utxo);
        self
    }

    /// Add the provided ProofOfWork metadata to the block
    pub fn with_pow(mut self, pow: ProofOfWork) -> Self {
        self.header.pow = pow;
        self
    }

    /// This will finish construction of the block and create the block
    pub fn build(self) -> Block {
        let mut block = Block {
            header: self.header,
            body: AggregateBody::new(self.inputs, self.outputs, self.kernels),
        };
        block.body.sort();
        block
    }
}

impl Hashable for Block {
    /// The block hash is just the header hash, since the inputs, outputs and range proofs are captured by their
    /// respective MMR roots in the header itself.
    fn hash(&self) -> Vec<u8> {
        // Note. If this changes, there will be a bug in chain_database::add_block_modifying_header
        self.header.hash()
    }
}

//----------------------------------------         Tests          ----------------------------------------------------//
