// Copyright 2019 The Exonum Team
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use exonum::helpers::Height;
use exonum::{explorer::CommittedTransaction, runtime::ErrorKind};
use exonum_btc_anchoring::{
    blockchain::{errors::Error, BtcAnchoringSchema},
    btc::{self, BuilderError},
    config::Config,
    test_helpers::{
        create_fake_funding_transaction, AnchoringTestKit, ANCHORING_INSTANCE_ID,
        ANCHORING_INSTANCE_NAME,
    },
};
use exonum_testkit::simple_supervisor::ConfigPropose;

fn assert_tx_error(tx: &CommittedTransaction, e: impl Into<ErrorKind>) {
    assert_eq!(tx.status().unwrap_err().kind, e.into(),);
}

fn unspent_funding_transaction(anchoring_testkit: &AnchoringTestKit) -> Option<btc::Transaction> {
    let snapshot = anchoring_testkit.inner.snapshot();
    BtcAnchoringSchema::new(ANCHORING_INSTANCE_NAME, &snapshot)
        .unspent_funding_transaction_entry()
        .get()
}

fn test_anchoring_config_change<F>(mut config_change_predicate: F) -> AnchoringTestKit
where
    F: FnMut(&mut AnchoringTestKit, &mut Config),
{
    let mut anchoring_testkit = AnchoringTestKit::default();
    let anchoring_interval = anchoring_testkit
        .actual_anchoring_config()
        .anchoring_interval;

    assert!(anchoring_testkit.last_anchoring_tx().is_none());
    // Establish anchoring transactions chain.
    anchoring_testkit.inner.create_block_with_transactions(
        anchoring_testkit
            .create_signature_txs()
            .into_iter()
            .flatten(),
    );

    // Skip the next anchoring height.
    anchoring_testkit
        .inner
        .create_blocks_until(Height(anchoring_interval * 2));

    let last_anchoring_tx = anchoring_testkit.last_anchoring_tx().unwrap();
    // Modify anchoring configuration.
    let mut new_cfg = anchoring_testkit.actual_anchoring_config();
    let old_cfg = new_cfg.clone();
    config_change_predicate(&mut anchoring_testkit, &mut new_cfg);

    // Commit configuration with without last anchoring node.
    anchoring_testkit.inner.create_block_with_transaction(
        ConfigPropose::actual_from(anchoring_testkit.inner.height().next())
            .service_config(ANCHORING_INSTANCE_ID, new_cfg.clone())
            .into_tx(),
    );

    // Extract a previous anchoring transaction from the proposal.
    let (anchoring_tx_proposal, previous_anchoring_tx) = anchoring_testkit
        .anchoring_transaction_proposal()
        .map(|(tx, inputs)| (tx, inputs[0].clone()))
        .unwrap();

    // Verify anchoring transaction proposal.
    {
        assert_eq!(last_anchoring_tx, previous_anchoring_tx);

        let snapshot = anchoring_testkit.inner.snapshot();
        let schema = BtcAnchoringSchema::new(ANCHORING_INSTANCE_NAME, &snapshot);
        assert_eq!(schema.following_config().unwrap(), new_cfg);
        assert_eq!(schema.actual_config(), old_cfg);

        let (out_script, payload) = anchoring_tx_proposal.anchoring_metadata().unwrap();
        // Height for the transition anchoring transaction should be same as in the latest
        // anchoring transaction.
        assert_eq!(payload.block_height, Height(0));
        assert_eq!(&new_cfg.anchoring_out_script(), out_script);
    }

    // Finalize transition transaction
    anchoring_testkit.inner.create_block_with_transactions(
        anchoring_testkit
            .create_signature_txs()
            .into_iter()
            .flatten(),
    );

    // Verify that the following configuration becomes an actual.
    let snapshot = anchoring_testkit.inner.snapshot();
    let schema = BtcAnchoringSchema::new(ANCHORING_INSTANCE_NAME, &snapshot);
    assert!(schema.following_config().is_none());
    assert_eq!(schema.actual_config(), new_cfg);

    assert_eq!(
        anchoring_tx_proposal.id(),
        anchoring_testkit.last_anchoring_tx().unwrap().id()
    );

    // Verify that we have an anchoring transaction proposal.
    let anchoring_tx_proposal = anchoring_testkit
        .anchoring_transaction_proposal()
        .unwrap()
        .0;
    // Verify anchoring transaction metadata
    let tx_meta = anchoring_tx_proposal.anchoring_metadata().unwrap();
    assert_eq!(tx_meta.1.block_height, Height(anchoring_interval));
    assert_eq!(
        anchoring_testkit
            .actual_anchoring_config()
            .anchoring_out_script(),
        *tx_meta.0
    );

    anchoring_testkit
}

#[test]
fn simple() {
    let mut anchoring_testkit = AnchoringTestKit::default();
    let anchoring_interval = anchoring_testkit
        .actual_anchoring_config()
        .anchoring_interval;

    assert!(anchoring_testkit.last_anchoring_tx().is_none());

    let signatures = anchoring_testkit
        .create_signature_txs()
        .into_iter()
        .flatten();

    anchoring_testkit
        .inner
        .create_block_with_transactions(signatures)
        .transactions
        .iter()
        .try_for_each(|tx| tx.status())
        .expect("Each transaction should be successful.");

    let tx0 = anchoring_testkit.last_anchoring_tx().unwrap();
    let tx0_meta = tx0.anchoring_metadata().unwrap();
    assert!(tx0_meta.1.block_height == Height(0));

    anchoring_testkit
        .inner
        .create_blocks_until(Height(anchoring_interval));

    // Ensure that the anchoring proposal has expected height.
    assert_eq!(
        anchoring_testkit
            .anchoring_transaction_proposal()
            .unwrap()
            .0
            .anchoring_payload()
            .unwrap()
            .block_height,
        Height(anchoring_interval)
    );

    let signatures = anchoring_testkit
        .create_signature_txs()
        .into_iter()
        .take(3)
        .flatten();
    anchoring_testkit
        .inner
        .create_block_with_transactions(signatures);

    let tx1 = anchoring_testkit.last_anchoring_tx().unwrap();
    let tx1_meta = tx1.anchoring_metadata().unwrap();

    assert!(tx0.id() == tx1.prev_tx_id());

    // script_pubkey should be the same
    assert!(tx0_meta.0 == tx1_meta.0);
    assert!(tx1_meta.1.block_height == Height(anchoring_interval));
}

#[test]
fn additional_funding() {
    let mut anchoring_testkit = AnchoringTestKit::default();
    let anchoring_interval = anchoring_testkit
        .actual_anchoring_config()
        .anchoring_interval;

    assert!(anchoring_testkit.last_anchoring_tx().is_none());
    // Establish anchoring transactions chain with the initial funding transaction.
    anchoring_testkit.inner.create_block_with_transactions(
        anchoring_testkit
            .create_signature_txs()
            .into_iter()
            .flatten(),
    );

    // Add another funding transaction.
    let (txs, new_funding_tx) = anchoring_testkit.create_funding_confirmation_txs(150_000);
    anchoring_testkit.inner.create_block_with_transactions(txs);

    // Reach the next anchoring height.
    anchoring_testkit
        .inner
        .create_blocks_until(Height(anchoring_interval));

    // Ensure that the anchoring proposal has input with the our funding transaction.
    assert_eq!(
        anchoring_testkit
            .anchoring_transaction_proposal()
            .unwrap()
            .1[1],
        new_funding_tx
    );

    anchoring_testkit.inner.create_block_with_transactions(
        anchoring_testkit
            .create_signature_txs()
            .into_iter()
            .take(3)
            .flatten(),
    );

    let tx1 = anchoring_testkit.last_anchoring_tx().unwrap();
    let tx1_meta = tx1.anchoring_metadata().unwrap();

    assert!(tx1_meta.1.block_height == Height(anchoring_interval));
    assert_eq!(tx1.0.input[1].previous_output.txid, new_funding_tx.0.txid());
}

#[test]
fn err_spent_funding() {
    let anchoring_interval = 5;
    let mut anchoring_testkit = AnchoringTestKit::new(4, anchoring_interval);

    // Add an initial funding transaction to enable anchoring.
    let (txs, spent_funding_transaction) =
        anchoring_testkit.create_funding_confirmation_txs(20_000);
    anchoring_testkit
        .inner
        .create_block_with_transactions(txs.into_iter().skip(1));

    assert!(anchoring_testkit.last_anchoring_tx().is_none());
    // Establish anchoring transactions chain with the initial funding transaction.
    anchoring_testkit.inner.create_block_with_transactions(
        anchoring_testkit
            .create_signature_txs()
            .into_iter()
            .flatten(),
    );

    // Try to add spent funding transaction.
    let block = anchoring_testkit.inner.create_block_with_transactions(
        anchoring_testkit
            .create_funding_confirmation_txs_with(spent_funding_transaction)
            .into_iter()
            .take(1),
    );
    assert_tx_error(&block[0], Error::AlreadyUsedFundingTx);

    // Reach the next anchoring height.
    anchoring_testkit
        .inner
        .create_blocks_until(Height(anchoring_interval));

    // Ensure that the anchoring proposal has no input with the spent funding transaction.
    assert_eq!(
        anchoring_testkit
            .anchoring_transaction_proposal()
            .unwrap()
            .1
            .len(),
        1
    );
}

#[test]
fn insufficient_funds() {
    let mut anchoring_testkit = AnchoringTestKit::new(4, 5);

    {
        let snapshot = anchoring_testkit.inner.snapshot();
        let schema = BtcAnchoringSchema::new(ANCHORING_INSTANCE_NAME, &snapshot);
        let proposal = schema.actual_proposed_anchoring_transaction().unwrap();
        assert_eq!(proposal, Err(BuilderError::NoInputs));
    }

    // Replenish the anchoring wallet by the given amount of satoshis.
    anchoring_testkit
        .inner
        .create_block_with_transactions(anchoring_testkit.create_funding_confirmation_txs(20).0);

    // Check that we have not enough satoshis to create proposal.
    {
        let snapshot = anchoring_testkit.inner.snapshot();
        let schema = BtcAnchoringSchema::new(ANCHORING_INSTANCE_NAME, &snapshot);
        let proposal = schema.actual_proposed_anchoring_transaction().unwrap();
        assert_eq!(
            proposal,
            Err(BuilderError::InsufficientFunds {
                balance: 20,
                total_fee: 1530
            })
        );
    }
}

#[test]
fn no_anchoring_proposal() {
    let mut anchoring_testkit = AnchoringTestKit::default();
    let anchoring_interval = anchoring_testkit
        .actual_anchoring_config()
        .anchoring_interval;

    assert!(anchoring_testkit.last_anchoring_tx().is_none());

    let mut signatures = anchoring_testkit.create_signature_txs();
    let leftover_signatures = signatures.pop().unwrap();

    anchoring_testkit
        .inner
        .create_block_with_transactions(signatures.into_iter().flatten());

    // Anchor a next height.
    anchoring_testkit
        .inner
        .create_blocks_until(Height(anchoring_interval));
    let signatures = anchoring_testkit
        .create_signature_txs()
        .into_iter()
        .flatten();
    anchoring_testkit
        .inner
        .create_block_with_transactions(signatures);

    // Very slow node.
    let block = anchoring_testkit
        .inner
        .create_block_with_transactions(leftover_signatures);
    assert_tx_error(&block[0], Error::UnexpectedProposalTxId);
}

#[test]
fn unexpected_anchoring_proposal() {
    let mut anchoring_testkit = AnchoringTestKit::default();
    let anchoring_interval = anchoring_testkit
        .actual_anchoring_config()
        .anchoring_interval;

    assert!(anchoring_testkit.last_anchoring_tx().is_none());

    let mut signatures = anchoring_testkit.create_signature_txs();
    let leftover_signatures = signatures.pop().unwrap();

    anchoring_testkit
        .inner
        .create_block_with_transactions(signatures.into_iter().flatten());

    anchoring_testkit
        .inner
        .create_blocks_until(Height(anchoring_interval));

    // Anchor a next height.
    let signatures = anchoring_testkit
        .create_signature_txs()
        .into_iter()
        .flatten();
    anchoring_testkit
        .inner
        .create_block_with_transactions(signatures);
    // Wait until the next anchoring height becomes an actual.
    anchoring_testkit
        .inner
        .create_blocks_until(Height(anchoring_interval * 2));

    // Very slow node
    let block = anchoring_testkit
        .inner
        .create_block_with_transactions(leftover_signatures);
    assert_tx_error(&block[0], Error::UnexpectedProposalTxId);
}

#[test]
fn add_anchoring_node() {
    test_anchoring_config_change(|anchoring_testkit, cfg| {
        cfg.anchoring_keys.push(anchoring_testkit.add_node());
    });
}

#[test]
fn remove_anchoring_node() {
    test_anchoring_config_change(|_, cfg| {
        cfg.anchoring_keys.pop();
    });
}

#[test]
fn change_anchoring_node_without_funds() {
    test_anchoring_config_change(|anchoring_testkit, cfg| {
        cfg.anchoring_keys[0].bitcoin_key = anchoring_testkit.gen_bitcoin_key();
    });
}

#[test]
fn add_anchoring_node_insufficient_funds() {
    let mut anchoring_testkit = AnchoringTestKit::new(4, 5);
    let anchoring_interval = anchoring_testkit
        .actual_anchoring_config()
        .anchoring_interval;

    // Add an initial funding transaction to enable anchoring.
    anchoring_testkit
        .inner
        .create_block_with_transactions(anchoring_testkit.create_funding_confirmation_txs(2000).0);

    assert!(anchoring_testkit.last_anchoring_tx().is_none());
    // Establish anchoring transactions chain.
    anchoring_testkit.inner.create_block_with_transactions(
        anchoring_testkit
            .create_signature_txs()
            .into_iter()
            .flatten(),
    );

    // Skip the next anchoring height.
    anchoring_testkit
        .inner
        .create_blocks_until(Height(anchoring_interval * 2));

    // Add an anchoring node.
    let mut new_cfg = anchoring_testkit.actual_anchoring_config();
    new_cfg.anchoring_keys.push(anchoring_testkit.add_node());

    // Commit configuration with without last anchoring node.
    anchoring_testkit.inner.create_block_with_transaction(
        ConfigPropose::actual_from(anchoring_testkit.inner.height().next())
            .service_config(ANCHORING_INSTANCE_ID, new_cfg.clone())
            .into_tx(),
    );
    anchoring_testkit.inner.create_block();

    // Ensure that the anchoring transaction proposal is unsuitable.
    {
        let snapshot = anchoring_testkit.inner.snapshot();
        let schema = BtcAnchoringSchema::new(ANCHORING_INSTANCE_NAME, &snapshot);
        let proposal = schema.actual_proposed_anchoring_transaction().unwrap();
        assert_eq!(
            proposal,
            Err(BuilderError::InsufficientFunds {
                total_fee: 1530,
                balance: 470
            })
        );
    }

    // Add funds.
    let (txs, funding_tx) = anchoring_testkit.create_funding_confirmation_txs(2000);
    anchoring_testkit.inner.create_block_with_transactions(txs);
    // Ensure that we have a suitable transition anchoring transaction proposal.
    assert_eq!(
        anchoring_testkit
            .anchoring_transaction_proposal()
            .unwrap()
            .1[1],
        funding_tx
    );
}

#[test]
fn funding_tx_err_unsuitable() {
    let mut anchoring_testkit = AnchoringTestKit::default();

    // Create weird funding transaction.
    let mut config = anchoring_testkit.actual_anchoring_config();
    config.anchoring_keys.swap(3, 1);
    let funding_tx = create_fake_funding_transaction(&config.anchoring_address(), 10_000);

    // Check that it did not pass.
    let block = anchoring_testkit.inner.create_block_with_transactions(
        anchoring_testkit.create_funding_confirmation_txs_with(funding_tx),
    );
    assert_tx_error(&block[0], Error::UnsuitableFundingTx);
}

#[test]
fn funding_tx_override() {
    // Actually, we can override the funding transaction by another one.
    let anchoring_interval = 5;
    let mut anchoring_testkit = AnchoringTestKit::new(4, anchoring_interval);

    // Add an initial funding transaction to enable anchoring.
    let (txs, first_funding_transaction) = anchoring_testkit.create_funding_confirmation_txs(2000);
    anchoring_testkit.inner.create_block_with_transactions(txs);
    assert_eq!(
        unspent_funding_transaction(&anchoring_testkit).unwrap(),
        first_funding_transaction
    );

    // Override the funding transaction by the second one.
    let (txs, second_funding_transaction) = anchoring_testkit.create_funding_confirmation_txs(2400);
    anchoring_testkit.inner.create_block_with_transactions(txs);
    assert_eq!(
        unspent_funding_transaction(&anchoring_testkit).unwrap(),
        second_funding_transaction
    );
}

// TODO Implement tests for anchoring recovery [ECR-3581]
