//  Copyright 2019 The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use crate::{
    blocks::Block,
    consts::MEMPOOL_PENDING_POOL_STORAGE_CAPACITY,
    mempool::pending_pool::{PendingPoolError, PendingPoolStorage},
    transactions::{transaction::Transaction, types::Signature},
};
use std::sync::{Arc, RwLock};

/// Configuration for the PendingPool.
#[derive(Clone, Copy)]
pub struct PendingPoolConfig {
    /// The maximum number of transactions that can be stored in the Pending pool.
    pub storage_capacity: usize,
}

impl Default for PendingPoolConfig {
    fn default() -> Self {
        Self {
            storage_capacity: MEMPOOL_PENDING_POOL_STORAGE_CAPACITY,
        }
    }
}

/// The Pending Pool contains all transactions that are restricted by time-locks. Once the time-locks have expired then
/// the transactions can be moved to the Unconfirmed Transaction Pool for inclusion in future blocks.
pub struct PendingPool {
    pool_storage: Arc<RwLock<PendingPoolStorage>>,
}

impl PendingPool {
    /// Create a new PendingPool with the specified configuration.
    pub fn new(config: PendingPoolConfig) -> Self {
        Self {
            pool_storage: Arc::new(RwLock::new(PendingPoolStorage::new(config))),
        }
    }

    /// Insert a new transaction into the PendingPool. Low priority transactions will be removed to make space for
    /// higher priority transactions. The lowest priority transactions will be removed when the maximum capacity is
    /// reached and the new transaction has a higher priority than the currently stored lowest priority transaction.
    pub fn insert(&self, transaction: Arc<Transaction>) -> Result<(), PendingPoolError> {
        self.pool_storage
            .write()
            .map_err(|_| PendingPoolError::PoisonedAccess)?
            .insert(transaction)
    }

    /// Insert a set of new transactions into the PendingPool.
    pub fn insert_txs(&self, transactions: Vec<Arc<Transaction>>) -> Result<(), PendingPoolError> {
        self.pool_storage
            .write()
            .map_err(|_| PendingPoolError::PoisonedAccess)?
            .insert_txs(transactions)
    }

    /// Check if a specific transaction is available in the PendingPool.
    pub fn has_tx_with_excess_sig(&self, excess_sig: &Signature) -> Result<bool, PendingPoolError> {
        Ok(self
            .pool_storage
            .read()
            .map_err(|_| PendingPoolError::PoisonedAccess)?
            .has_tx_with_excess_sig(excess_sig))
    }

    /// Remove transactions with expired time-locks so that they can be move to the UnconfirmedPool. Double spend
    /// transactions are also removed.
    pub fn remove_unlocked_and_discard_double_spends(
        &self,
        published_block: &Block,
    ) -> Result<Vec<Arc<Transaction>>, PendingPoolError>
    {
        self.pool_storage
            .write()
            .map_err(|_| PendingPoolError::PoisonedAccess)?
            .remove_unlocked_and_discard_double_spends(published_block)
    }

    /// Returns the total number of time-locked transactions stored in the PendingPool.
    pub fn len(&self) -> Result<usize, PendingPoolError> {
        Ok(self
            .pool_storage
            .read()
            .map_err(|_| PendingPoolError::PoisonedAccess)?
            .len())
    }

    /// Returns all transaction stored in the PendingPool.
    pub fn snapshot(&self) -> Result<Vec<Arc<Transaction>>, PendingPoolError> {
        Ok(self
            .pool_storage
            .read()
            .map_err(|_| PendingPoolError::PoisonedAccess)?
            .snapshot())
    }

    /// Returns the total weight of all transactions stored in the pool.
    pub fn calculate_weight(&self) -> Result<u64, PendingPoolError> {
        Ok(self
            .pool_storage
            .read()
            .map_err(|_| PendingPoolError::PoisonedAccess)?
            .calculate_weight())
    }

    #[cfg(test)]
    /// Checks the consistency status of the Hashmap and BtreeMaps.
    pub fn check_status(&self) -> Result<bool, PendingPoolError> {
        Ok(self
            .pool_storage
            .read()
            .map_err(|_| PendingPoolError::PoisonedAccess)?
            .check_status())
    }
}

impl Clone for PendingPool {
    fn clone(&self) -> Self {
        PendingPool {
            pool_storage: self.pool_storage.clone(),
        }
    }
}

#[cfg(test)]
mod test {
    use crate::{
        helpers::create_orphan_block,
        mempool::pending_pool::{PendingPool, PendingPoolConfig},
        transactions::tari_amount::MicroTari,
        tx,
    };
    use std::sync::Arc;

    #[test]
    fn test_insert_and_lru() {
        let tx1 = Arc::new(tx!(MicroTari(10_000), fee: MicroTari(50), lock: 500, inputs: 2, outputs: 1).0);
        let tx2 = Arc::new(tx!(MicroTari(10_000), fee: MicroTari(20), lock: 2150, inputs: 1, outputs: 2).0);
        let tx3 = Arc::new(tx!(MicroTari(10_000), fee: MicroTari(100), lock: 1000, inputs: 2, outputs: 1).0);
        let tx4 = Arc::new(tx!(MicroTari(10_000), fee: MicroTari(30), lock: 2450, inputs: 2, outputs: 2).0);
        let tx5 = Arc::new(tx!(MicroTari(10_000), fee: MicroTari(50), lock: 1000, inputs: 3, outputs: 3).0);
        let tx6 = Arc::new(tx!(MicroTari(10_000), fee: MicroTari(75), lock: 1850, inputs: 2, outputs: 2).0);

        let pending_pool = PendingPool::new(PendingPoolConfig { storage_capacity: 3 });
        pending_pool
            .insert_txs(vec![
                tx1.clone(),
                tx2.clone(),
                tx3.clone(),
                tx4.clone(),
                tx5.clone(),
                tx6.clone(),
            ])
            .unwrap();
        // Check that lowest priority txs were removed to make room for higher priority transactions
        assert_eq!(pending_pool.len().unwrap(), 3);
        assert_eq!(
            pending_pool
                .has_tx_with_excess_sig(&tx1.body.kernels()[0].excess_sig)
                .unwrap(),
            true
        );
        assert_eq!(
            pending_pool
                .has_tx_with_excess_sig(&tx2.body.kernels()[0].excess_sig)
                .unwrap(),
            false
        );
        assert_eq!(
            pending_pool
                .has_tx_with_excess_sig(&tx3.body.kernels()[0].excess_sig)
                .unwrap(),
            true
        );
        assert_eq!(
            pending_pool
                .has_tx_with_excess_sig(&tx4.body.kernels()[0].excess_sig)
                .unwrap(),
            false
        );
        assert_eq!(
            pending_pool
                .has_tx_with_excess_sig(&tx5.body.kernels()[0].excess_sig)
                .unwrap(),
            false
        );
        assert_eq!(
            pending_pool
                .has_tx_with_excess_sig(&tx6.body.kernels()[0].excess_sig)
                .unwrap(),
            true
        );

        assert!(pending_pool.check_status().unwrap());
    }

    #[test]
    fn test_remove_unlocked_and_discard_double_spends() {
        let tx1 = Arc::new(tx!(MicroTari(10_000), fee: MicroTari(50), lock: 500, inputs: 2, outputs: 1).0);
        let tx2 =
            Arc::new(tx!(MicroTari(10_000), fee: MicroTari(20), lock: 0, inputs: 1, maturity: 2150, outputs: 2).0);
        let tx3 = Arc::new(
            tx!(MicroTari(10_000), fee: MicroTari(100), lock: 0, inputs: 2, maturity: 1000, outputs:
        1)
            .0,
        );
        let tx4 = Arc::new(tx!(MicroTari(10_000), fee: MicroTari(30), lock: 2450, inputs: 2, outputs: 2).0);
        let tx5 = Arc::new(tx!(MicroTari(10_000), fee: MicroTari(50), lock: 1000, inputs: 3, outputs: 3).0);
        let tx6 =
            Arc::new(tx!(MicroTari(10_000), fee: MicroTari(75), lock: 1450, inputs: 2, maturity: 1400, outputs: 2).0);

        let pending_pool = PendingPool::new(PendingPoolConfig { storage_capacity: 10 });
        pending_pool
            .insert_txs(vec![
                tx1.clone(),
                tx2.clone(),
                tx3.clone(),
                tx4.clone(),
                tx5.clone(),
                tx6.clone(),
            ])
            .unwrap();
        assert_eq!(pending_pool.len().unwrap(), 6);

        let snapshot_txs = pending_pool.snapshot().unwrap();
        assert_eq!(snapshot_txs.len(), 6);
        assert!(snapshot_txs.contains(&tx1));
        assert!(snapshot_txs.contains(&tx2));
        assert!(snapshot_txs.contains(&tx3));
        assert!(snapshot_txs.contains(&tx4));
        assert!(snapshot_txs.contains(&tx5));
        assert!(snapshot_txs.contains(&tx6));

        let published_block = create_orphan_block(1500, vec![(*tx6).clone()]);
        let unlocked_txs = pending_pool
            .remove_unlocked_and_discard_double_spends(&published_block)
            .unwrap();

        assert_eq!(pending_pool.len().unwrap(), 2);
        assert_eq!(
            pending_pool
                .has_tx_with_excess_sig(&tx2.body.kernels()[0].excess_sig)
                .unwrap(),
            true
        );
        assert_eq!(
            pending_pool
                .has_tx_with_excess_sig(&tx4.body.kernels()[0].excess_sig)
                .unwrap(),
            true
        );

        assert_eq!(unlocked_txs.len(), 3);
        assert!(unlocked_txs.contains(&tx1));
        assert!(unlocked_txs.contains(&tx3));
        assert!(unlocked_txs.contains(&tx5));

        assert!(pending_pool.check_status().unwrap());
    }
}
