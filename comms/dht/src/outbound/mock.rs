// Copyright 2019, The Tari Project
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

use crate::outbound::{
    message::SendMessageResponse,
    message_params::FinalSendMessageParams,
    DhtOutboundRequest,
    OutboundMessageRequester,
};
use futures::{channel::mpsc, stream::Fuse, StreamExt};
use std::{
    sync::{Arc, Condvar, Mutex, RwLock},
    time::Duration,
};
use tari_comms::condvar_shim;

/// Creates a mock outbound request "handler" for testing purposes.
///
/// Each time a request is expected, handle_next should be called.
pub fn create_outbound_service_mock(size: usize) -> (OutboundMessageRequester, OutboundServiceMock) {
    let (tx, rx) = mpsc::channel(size);
    (OutboundMessageRequester::new(tx), OutboundServiceMock::new(rx.fuse()))
}

#[derive(Clone)]
pub struct OutboundServiceMockState {
    calls: Arc<Mutex<Vec<(FinalSendMessageParams, Vec<u8>)>>>,
    next_response: Arc<RwLock<Option<SendMessageResponse>>>,
    call_count_cond_var: Arc<Condvar>,
}

impl OutboundServiceMockState {
    pub fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            next_response: Arc::new(RwLock::new(None)),
            call_count_cond_var: Arc::new(Condvar::new()),
        }
    }

    pub fn call_count(&self) -> usize {
        acquire_lock!(self.calls).len()
    }

    /// Wait for `num_calls` extra calls or timeout.
    ///
    /// An error will be returned if the timeout expires.
    pub fn wait_call_count(&self, expected_calls: usize, timeout: Duration) -> Result<usize, String> {
        let call_guard = acquire_lock!(self.calls);
        let (call_guard, is_timeout) =
            condvar_shim::wait_timeout_until(&self.call_count_cond_var, call_guard, timeout, |calls| {
                calls.len() >= expected_calls
            })
            .expect("CondVar must never be poisoned");

        if is_timeout {
            Err(format!(
                "wait_call_count timed out before before receiving the expected number of calls. (Expected = {}, Got \
                 = {})",
                expected_calls,
                call_guard.len()
            ))
        } else {
            Ok(call_guard.len())
        }
    }

    /// Wait for a call to be added or timeout.
    ///
    /// An error will be returned if the timeout expires.
    pub fn wait_pop_call(&self, timeout: Duration) -> Result<(FinalSendMessageParams, Vec<u8>), String> {
        let call_guard = acquire_lock!(self.calls);
        let (mut call_guard, timeout) = self
            .call_count_cond_var
            .wait_timeout(call_guard, timeout)
            .expect("CondVar must never be poisoned");

        if timeout.timed_out() {
            Err("wait_pop_call timed out before before receiving a call.".to_string())
        } else {
            Ok(call_guard.pop().expect("calls.len() must be greater than 1"))
        }
    }

    pub fn take_next_response(&self) -> Option<SendMessageResponse> {
        acquire_write_lock!(self.next_response).take()
    }

    pub fn add_call(&self, req: (FinalSendMessageParams, Vec<u8>)) {
        acquire_lock!(self.calls).push(req);
        self.call_count_cond_var.notify_all();
    }

    pub fn take_calls(&self) -> Vec<(FinalSendMessageParams, Vec<u8>)> {
        acquire_lock!(self.calls).drain(..).collect()
    }

    pub fn pop_call(&self) -> Option<(FinalSendMessageParams, Vec<u8>)> {
        acquire_lock!(self.calls).pop()
    }
}

pub struct OutboundServiceMock {
    receiver: Fuse<mpsc::Receiver<DhtOutboundRequest>>,
    mock_state: OutboundServiceMockState,
}

impl OutboundServiceMock {
    pub fn new(receiver: Fuse<mpsc::Receiver<DhtOutboundRequest>>) -> Self {
        Self {
            receiver,
            mock_state: OutboundServiceMockState::new(),
        }
    }

    pub fn get_state(&self) -> OutboundServiceMockState {
        self.mock_state.clone()
    }

    pub async fn run(mut self) {
        while let Some(req) = self.receiver.next().await {
            match req {
                DhtOutboundRequest::SendMessage(params, body, reply_tx) => {
                    self.mock_state.add_call((*params, body));
                    let response = self
                        .mock_state
                        .take_next_response()
                        .or(Some(SendMessageResponse::Ok(0)))
                        .expect("never none");

                    reply_tx.send(response).expect("Reply channel cancelled");
                },
            }
        }
    }
}
