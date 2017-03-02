pub use bitcoinrpc::RpcError as JsonRpcError;
pub use bitcoinrpc::Error as RpcError;

use serde_json::value::ToJson;
use bitcoin::util::base58::FromBase58;

use exonum::messages::{Message, RawTransaction};
use exonum::crypto::HexValue;

use sandbox::sandbox::Sandbox;
use sandbox::sandbox_tests_helper::{SandboxState, add_one_height_with_transactions};
use sandbox::config_updater::TxConfig;

use anchoring_service::sandbox::{SandboxClient, Request};
use anchoring_service::{ANCHORING_SERVICE, TxAnchoringUpdateLatest};
use anchoring_service::transactions::{BitcoinTx, RawBitcoinTx};
use anchoring_service::btc::TxId;
use anchoring_service::config::AnchoringConfig;
use anchoring_service::btc;

use AnchoringSandboxState;

pub fn gen_service_tx_lect(sandbox: &Sandbox,
                           validator: u32,
                           tx: &RawBitcoinTx,
                           prev_hash: &TxId)
                           -> RawTransaction {
    let tx = TxAnchoringUpdateLatest::new(sandbox.p(validator as usize),
                                          validator,
                                          BitcoinTx::from(tx.clone()),
                                          &prev_hash,
                                          sandbox.s(validator as usize));
    tx.raw().clone()
}

pub fn gen_update_config_tx(sandbox: &Sandbox,
                            actual_from: u64,
                            service_cfg: AnchoringConfig)
                            -> RawTransaction {
    let mut cfg = sandbox.cfg();
    *cfg.services.get_mut(&ANCHORING_SERVICE).unwrap() = service_cfg.to_json();
    let tx = TxConfig::new(sandbox.p(0), &cfg.serialize(), actual_from, sandbox.s(0));
    tx.raw().clone()
}

/// Anchor genesis block using funding tx
pub fn anchor_first_block(sandbox: &Sandbox,
                          client: &SandboxClient,
                          sandbox_state: &SandboxState,
                          anchoring_state: &mut AnchoringSandboxState) {
    client.expect(vec![request! {
            method: "listunspent",
            params: [0, 9999999, ["2NAkCcmVunAzQvKFgyQDbCApuKd9xwN6SRu"]],
            response: [
                {
                    "txid": "a03b10b17fc8b86dd0b1b6ebcc3bc3c6dd4b7173302ef68628f5ed768dbd7049",
                    "vout": 0,
                    "address": "2NAkCcmVunAzQvKFgyQDbCApuKd9xwN6SRu",
                    "account": "multisig",
                    "scriptPubKey": "a914499d997314d6e55e49293b50d8dfb78bb9c958ab87",
                    "amount": 0.00010000,
                    "confirmations": 50,
                    "spendable": false,
                    "solvable": false
                }
            ]
        }]);

    let (_, signatures) = anchoring_state.gen_anchoring_tx_with_signatures(sandbox,
        0,
        sandbox.last_hash(),
        &[],
        &btc::Address::from_base58check("2NAkCcmVunAzQvKFgyQDbCApuKd9xwN6SRu").unwrap()
    );
    let anchored_tx = anchoring_state.latest_anchored_tx();
    add_one_height_with_transactions(&sandbox, &sandbox_state, &[]);

    sandbox.broadcast(signatures[0].clone());
    client.expect(vec![// TODO add support for error response
                       Request {
                           method: "getrawtransaction",
                           params: vec![anchored_tx.txid().to_json(), 1.to_json()],
                           response: Err(RpcError::NoInformation("Unable to find tx".to_string())),
                       },
                       request! {
            method: "sendrawtransaction",
            params: [anchored_tx.to_hex()]
        }]);

    let signatures = signatures.into_iter()
        .map(|tx| tx.raw().clone())
        .collect::<Vec<_>>();
    add_one_height_with_transactions(&sandbox, &sandbox_state, &signatures);

    let txs = [gen_service_tx_lect(sandbox, 0, &anchored_tx, &anchored_tx.prev_hash()),
               gen_service_tx_lect(sandbox, 1, &anchored_tx, &anchored_tx.prev_hash()),
               gen_service_tx_lect(sandbox, 2, &anchored_tx, &anchored_tx.prev_hash()),
               gen_service_tx_lect(sandbox, 3, &anchored_tx, &anchored_tx.prev_hash())];

    sandbox.broadcast(txs[0].raw().clone());
    let txs = txs.into_iter()
        .map(|x| x.raw())
        .cloned()
        .collect::<Vec<_>>();
    add_one_height_with_transactions(sandbox, sandbox_state, &txs);
}

pub fn anchor_first_block_lect_normal(sandbox: &Sandbox,
                                      client: &SandboxClient,
                                      sandbox_state: &SandboxState,
                                      anchoring_state: &mut AnchoringSandboxState) {
    // Just add few heights
    add_one_height_with_transactions(sandbox, sandbox_state, &[]);
    add_one_height_with_transactions(&sandbox, &sandbox_state, &[]);

    let anchored_tx = anchoring_state.latest_anchored_tx();

    client.expect(vec![request! {
            method: "listunspent",
            params: [0, 9999999, ["2NAkCcmVunAzQvKFgyQDbCApuKd9xwN6SRu"]],
            response: [
                {
                    "txid": &anchored_tx.txid(),
                    "vout": 0,
                    "address": "2NAkCcmVunAzQvKFgyQDbCApuKd9xwN6SRu",
                    "account": "multisig",
                    "scriptPubKey": "a914499d997314d6e55e49293b50d8dfb78bb9c958ab87",
                    "amount": 0.00010000,
                    "confirmations": 0,
                    "spendable": false,
                    "solvable": false
                }
            ]
        },
                       request! {
            method: "getrawtransaction",
            params: [&anchored_tx.txid(), 0],
            response: &anchored_tx.to_hex()
        }]);
    add_one_height_with_transactions(sandbox, sandbox_state, &[]);
}

pub fn anchor_first_block_lect_lost(sandbox: &Sandbox,
                                    client: &SandboxClient,
                                    sandbox_state: &SandboxState,
                                    anchoring_state: &mut AnchoringSandboxState) {
    anchor_first_block(sandbox, client, sandbox_state, anchoring_state);
    // Just add few heights
    add_one_height_with_transactions(sandbox, sandbox_state, &[]);
    add_one_height_with_transactions(sandbox, sandbox_state, &[]);

    let lost_lect_id = anchoring_state.latest_anchored_tx().id();
    let other_lect = anchoring_state.genesis.funding_tx.clone();

    client.expect(vec![request! {
            method: "listunspent",
            params: [0, 9999999, ["2NAkCcmVunAzQvKFgyQDbCApuKd9xwN6SRu"]],
            response: [
                {
                    "txid": &other_lect.txid(),
                    "vout": 0,
                    "address": "2NAkCcmVunAzQvKFgyQDbCApuKd9xwN6SRu",
                    "account": "multisig",
                    "scriptPubKey": "a914499d997314d6e55e49293b50d8dfb78bb9c958ab87",
                    "amount": 0.00010000,
                    "confirmations": 0,
                    "spendable": false,
                    "solvable": false
                }
            ]
        },
                       request! {
            method: "getrawtransaction",
            params: [&other_lect.txid(), 0],
            response: &other_lect.to_hex()
        }]);
    add_one_height_with_transactions(sandbox, sandbox_state, &[]);

    let txs = [gen_service_tx_lect(sandbox, 0, &other_lect, &lost_lect_id),
               gen_service_tx_lect(sandbox, 1, &other_lect, &lost_lect_id),
               gen_service_tx_lect(sandbox, 2, &other_lect, &lost_lect_id),
               gen_service_tx_lect(sandbox, 3, &other_lect, &lost_lect_id)];

    sandbox.broadcast(txs[0].raw().clone());
    let txs = txs.into_iter()
        .map(|x| x.raw())
        .cloned()
        .collect::<Vec<_>>();

    client.expect(vec![request! {
            method: "listunspent",
            params: [0, 9999999, ["2NAkCcmVunAzQvKFgyQDbCApuKd9xwN6SRu"]],
            response: [
                {
                    "txid": &other_lect.txid(),
                    "vout": 0,
                    "address": "2NAkCcmVunAzQvKFgyQDbCApuKd9xwN6SRu",
                    "account": "multisig",
                    "scriptPubKey": "a914499d997314d6e55e49293b50d8dfb78bb9c958ab87",
                    "amount": 0.00010000,
                    "confirmations": 100,
                    "spendable": false,
                    "solvable": false
                }
            ]
        }]);
    add_one_height_with_transactions(sandbox, sandbox_state, &txs);

    {
        let anchored_tx = anchoring_state.latest_anchored_tx();

        client.expect(vec![// TODO add support for error response
                           Request {
                               method: "getrawtransaction",
                               params: vec![anchored_tx.txid().to_json(), 1.to_json()],
                               response: Err(RpcError::NoInformation("Unable to find tx"
                                   .to_string())),
                           },
                           request! {
                method: "sendrawtransaction",
                params: [anchored_tx.to_hex()]
            }]);
        add_one_height_with_transactions(&sandbox, &sandbox_state, &[]);
    }
    anchoring_state.latest_anchored_tx = None;
}

pub fn anchor_first_block_lect_different(sandbox: &Sandbox,
                                         client: &SandboxClient,
                                         sandbox_state: &SandboxState,
                                         anchoring_state: &mut AnchoringSandboxState) {
    anchor_first_block(sandbox, client, sandbox_state, anchoring_state);
    // Just add few heights
    add_one_height_with_transactions(sandbox, sandbox_state, &[]);
    add_one_height_with_transactions(sandbox, sandbox_state, &[]);

    let lost_lect_id = anchoring_state.latest_anchored_tx().id();
    let (other_lect, other_signatures) = {
        let anchored_tx = anchoring_state.latest_anchored_tx();
        let other_signatures = anchoring_state.latest_anchored_tx_signatures()
            .iter()
            .filter(|tx| tx.validator() != 0)
            .cloned()
            .collect::<Vec<_>>();
        let other_lect =
            anchoring_state.finalize_tx(anchored_tx.clone(), other_signatures.as_ref());
        (other_lect, other_signatures)
    };

    client.expect(vec![request! {
            method: "listunspent",
            params: [0, 9999999, ["2NAkCcmVunAzQvKFgyQDbCApuKd9xwN6SRu"]],
            response: [
                {
                    "txid": &other_lect.txid(),
                    "vout": 0,
                    "address": "2NAkCcmVunAzQvKFgyQDbCApuKd9xwN6SRu",
                    "account": "multisig",
                    "scriptPubKey": "a914499d997314d6e55e49293b50d8dfb78bb9c958ab87",
                    "amount": 0.00010000,
                    "confirmations": 0,
                    "spendable": false,
                    "solvable": false
                }
            ]
        },
                       request! {
            method: "getrawtransaction",
            params: [&other_lect.txid(), 0],
            response: &other_lect.to_hex()
        }]);
    add_one_height_with_transactions(sandbox, sandbox_state, &[]);

    let txs = [gen_service_tx_lect(sandbox, 0, &other_lect, &lost_lect_id),
               gen_service_tx_lect(sandbox, 1, &other_lect, &lost_lect_id),
               gen_service_tx_lect(sandbox, 2, &other_lect, &lost_lect_id),
               gen_service_tx_lect(sandbox, 3, &other_lect, &lost_lect_id)];

    sandbox.broadcast(txs[0].raw().clone());
    let txs = txs.into_iter()
        .map(|x| x.raw())
        .cloned()
        .collect::<Vec<_>>();

    add_one_height_with_transactions(sandbox, sandbox_state, &txs);
    anchoring_state.latest_anchored_tx = Some((other_lect.clone(), other_signatures.clone()));
}

pub fn anchor_second_block_normal(sandbox: &Sandbox,
                                  client: &SandboxClient,
                                  sandbox_state: &SandboxState,
                                  anchoring_state: &mut AnchoringSandboxState) {
    add_one_height_with_transactions(sandbox, sandbox_state, &[]);
    add_one_height_with_transactions(sandbox, sandbox_state, &[]);
    add_one_height_with_transactions(sandbox, sandbox_state, &[]);

    client.expect(vec![request! {
            method: "listunspent",
            params: [0, 9999999, ["2NAkCcmVunAzQvKFgyQDbCApuKd9xwN6SRu"]],
            response: [
                {
                    "txid": "fea0a60f7146e7facf5bb382b80dafb762175bf0d4b6ac4e59c09cd4214d1491",
                    "vout": 0,
                    "address": "2NAkCcmVunAzQvKFgyQDbCApuKd9xwN6SRu",
                    "account": "multisig",
                    "scriptPubKey": "a914499d997314d6e55e49293b50d8dfb78bb9c958ab87",
                    "amount": 0.00010000,
                    "confirmations": 1,
                    "spendable": false,
                    "solvable": false
                }
            ]
        }]);
    add_one_height_with_transactions(sandbox, sandbox_state, &[]);

    let (_, signatures) = anchoring_state.gen_anchoring_tx_with_signatures(sandbox,
        10,
        sandbox.last_hash(),
        &[],
        &btc::Address::from_base58check("2NAkCcmVunAzQvKFgyQDbCApuKd9xwN6SRu").unwrap()
    );
    let anchored_tx = anchoring_state.latest_anchored_tx();

    sandbox.broadcast(signatures[0].clone());

    client.expect(vec![
        request! {
            method: "getrawtransaction",
            params: [&anchored_tx.txid(), 1],
            response: {
                "hash":&anchored_tx.txid(),"hex":&anchored_tx.to_hex(),
                "locktime":1088682,"size":223,"txid":"4ae2de1782b19ddab252d88d570f60bc821bd745d031029a8b28f7427c8d0e93","version":1,"vin":[{"scriptSig":{"asm":"3044022075b9f164d9fe44c348c7a18381314c3e6cf22c48e08bacc2ac6e145fd28f73800220448290b7c54ae465a34bb64a1427794428f7d99cc73204a5e501541d07b33e8a[ALL] 02c5f412387bffcc44dec76b28b948bfd7483ec939858c4a65bace07794e97f876","hex":"473044022075b9f164d9fe44c348c7a18381314c3e6cf22c48e08bacc2ac6e145fd28f73800220448290b7c54ae465a34bb64a1427794428f7d99cc73204a5e501541d07b33e8a012102c5f412387bffcc44dec76b28b948bfd7483ec939858c4a65bace07794e97f876"},"sequence":429496729,"txid":"094d7f6acedd8eb4f836ff483157a97155373974ac0ba3278a60e7a0a5efd645","vout":0}],"vout":[{"n":0,"scriptPubKey":{"addresses":["2NDG2AbxE914amqvimARQF2JJBZ9vHDn3Ga"],"asm":"OP_HASH160 db891024f2aa265e3b1998617e8b18ed3b0495fc OP_EQUAL","hex":"a914db891024f2aa265e3b1998617e8b18ed3b0495fc87","reqSigs":1,"type":"scripthash"},"value":0.00004},{"n":1,"scriptPubKey":{"addresses":["mn1jSMdewrpxTDkg1N6brC7fpTNV9X2Cmq"],"asm":"OP_DUP OP_HASH160 474215d1e614a7d9dddbd853d9f139cff2e99e1a OP_EQUALVERIFY OP_CHECKSIG","hex":"76a914474215d1e614a7d9dddbd853d9f139cff2e99e1a88ac","reqSigs":1,"type":"pubkeyhash"},"value":1.00768693}],"vsize":223
                }
        }
    ]);

    let signatures = signatures.into_iter()
        .map(|tx| tx.raw().clone())
        .collect::<Vec<_>>();
    add_one_height_with_transactions(sandbox, sandbox_state, &signatures);

    let txs = [gen_service_tx_lect(sandbox, 0, &anchored_tx, &anchored_tx.prev_hash()),
               gen_service_tx_lect(sandbox, 1, &anchored_tx, &anchored_tx.prev_hash()),
               gen_service_tx_lect(sandbox, 2, &anchored_tx, &anchored_tx.prev_hash()),
               gen_service_tx_lect(sandbox, 3, &anchored_tx, &anchored_tx.prev_hash())];

    sandbox.broadcast(txs[0].clone());
    client.expect(vec![request! {
            method: "listunspent",
            params: [0, 9999999, ["2NAkCcmVunAzQvKFgyQDbCApuKd9xwN6SRu"]],
            response: [
                {
                    "txid": &anchored_tx.txid(),
                    "vout": 0,
                    "address": "2NAkCcmVunAzQvKFgyQDbCApuKd9xwN6SRu",
                    "account": "multisig",
                    "scriptPubKey": "a914499d997314d6e55e49293b50d8dfb78bb9c958ab87",
                    "amount": 0.00010000,
                    "confirmations": 100,
                    "spendable": false,
                    "solvable": false
                }
            ]
        },
                       request! {
            method: "getrawtransaction",
            params: [&anchored_tx.txid(), 0],
            response: &anchored_tx.to_hex()
        }]);

    let txs = txs.into_iter()
        .map(|tx| tx.raw().clone())
        .collect::<Vec<_>>();
    add_one_height_with_transactions(sandbox, sandbox_state, &txs);
}