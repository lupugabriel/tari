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
    mempool::{
        mempool::{StatsResponse, TxStorageResponse},
        service::{MempoolRequest, MempoolResponse, MempoolServiceError},
    },
    transactions::{transaction::Transaction, types::Signature},
};
use futures::channel::mpsc::UnboundedSender;
use tari_comms::types::CommsPublicKey;
use tari_service_framework::reply_channel::SenderService;
use tower_service::Service;

/// The OutboundMempoolServiceInterface provides an interface to request information from the Mempools of remote Base
/// nodes.
#[derive(Clone)]
pub struct OutboundMempoolServiceInterface {
    request_sender: SenderService<MempoolRequest, Result<MempoolResponse, MempoolServiceError>>,
    tx_sender: UnboundedSender<(Transaction, Vec<CommsPublicKey>)>,
}

impl OutboundMempoolServiceInterface {
    /// Construct a new OutboundMempoolServiceInterface with the specified SenderService.
    pub fn new(
        request_sender: SenderService<MempoolRequest, Result<MempoolResponse, MempoolServiceError>>,
        tx_sender: UnboundedSender<(Transaction, Vec<CommsPublicKey>)>,
    ) -> Self
    {
        Self {
            request_sender,
            tx_sender,
        }
    }

    /// Request the stats from the mempool of a remote base node.
    pub async fn get_stats(&mut self) -> Result<StatsResponse, MempoolServiceError> {
        if let MempoolResponse::Stats(stats) = self.request_sender.call(MempoolRequest::GetStats).await?? {
            Ok(stats)
        } else {
            Err(MempoolServiceError::UnexpectedApiResponse)
        }
    }

    /// Transmit a transaction to remote base nodes, excluding the provided peers.
    pub async fn propagate_tx(
        &mut self,
        transaction: Transaction,
        exclude_peers: Vec<CommsPublicKey>,
    ) -> Result<(), MempoolServiceError>
    {
        self.tx_sender
            .unbounded_send((transaction, exclude_peers))
            .map_err(|_| MempoolServiceError::BroadcastFailed)
    }

    /// Check if the specified transaction is stored in the mempool of a remote base node.
    pub async fn get_tx_state_with_excess_sig(
        &mut self,
        excess_sig: Signature,
    ) -> Result<TxStorageResponse, MempoolServiceError>
    {
        if let MempoolResponse::TxStorage(tx_storage_response) = self
            .request_sender
            .call(MempoolRequest::GetTxStateWithExcessSig(excess_sig))
            .await??
        {
            Ok(tx_storage_response)
        } else {
            Err(MempoolServiceError::UnexpectedApiResponse)
        }
    }
}
