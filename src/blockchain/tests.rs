use exonum::crypto::{PublicKey, Signature, HexValue};

use bitcoin::blockdata::transaction::SigHashType;
use bitcoin::network::constants::Network;

use details::btc;
use details::btc::transactions::AnchoringTx;
use blockchain::dto::MsgAnchoringSignature;
use details::tests::{dummy_anchoring_tx, gen_anchoring_keys, make_signatures};

#[test]
fn test_sighash_type_all_in_msg_signature() {
    let tx = AnchoringTx::from_hex("01000000019aaf09d7e73a5f9ab394f1358bfb3dbde7b15b983d715f5c98f369a3f0a288a70000000000ffffffff02b80b00000000000017a914f18eb74087f751109cc9052befd4177a52c9a30a8700000000000000002c6a2a012800000000000000007fab6f66a0f7a747c820cd01fa30d7bdebd26b91c6e03f742abac0b3108134d900000000").unwrap();
    let btc_signature = btc::Signature::from_hex("3044022061d0bd408ec10f4f901c6d548151cc53031a3083f28dbcfc132319a162421d24022074f8a1c182088389bfae8646d9d99dea5b47db8f795d02efcc41ab4da0a8e11b01").unwrap();
    let msg = MsgAnchoringSignature::new_with_signature(&PublicKey::zero(),
                                                        0,
                                                        tx,
                                                        0,
                                                        &btc_signature,
                                                        &Signature::zero());

    assert!(msg.verify_content());
}

#[test]
fn test_sighash_type_single_in_msg_signature() {
    let tx = AnchoringTx::from_hex("01000000019aaf09d7e73a5f9ab394f1358bfb3dbde7b15b983d715f5c98f369a3f0a288a70000000000ffffffff02b80b00000000000017a914f18eb74087f751109cc9052befd4177a52c9a30a8700000000000000002c6a2a012800000000000000007fab6f66a0f7a747c820cd01fa30d7bdebd26b91c6e03f742abac0b3108134d900000000").unwrap();
    let mut btc_signature = btc::Signature::from_hex("3044022061d0bd408ec10f4f901c6d548151cc53031a3083f28dbcfc132319a162421d24022074f8a1c182088389bfae8646d9d99dea5b47db8f795d02efcc41ab4da0a8e11b01").unwrap();
    *btc_signature.last_mut().unwrap() = SigHashType::Single.as_u32() as u8;

    let msg = MsgAnchoringSignature::new_with_signature(&PublicKey::zero(),
                                                        0,
                                                        tx,
                                                        0,
                                                        &btc_signature,
                                                        &Signature::zero());
    assert!(!msg.verify_content());
}

#[test]
fn test_signed_input_in_msg_signature_tx_body() {
    let (pub_keys, priv_keys) = gen_anchoring_keys(4);
    let redeem_script = btc::RedeemScript::from_pubkeys(&pub_keys, 3).compressed(Network::Bitcoin);

    let tx = dummy_anchoring_tx(&redeem_script);
    let btc_signatures = make_signatures(&redeem_script, &tx, &[0], &priv_keys);
    let signed_tx = tx.clone()
        .finalize(&redeem_script, btc_signatures.clone());

    assert!(signed_tx.nid() != signed_tx.id());
    assert_eq!(signed_tx.nid(), tx.id());

    let msg = MsgAnchoringSignature::new_with_signature(&PublicKey::zero(),
                                                        0,
                                                        signed_tx,
                                                        0,
                                                        &btc_signatures[&0][0],
                                                        &Signature::zero());
    assert!(!msg.verify_content());
}

#[test]
fn test_nonexistent_input_in_msg_signature_tx_body() {
    let (pub_keys, priv_keys) = gen_anchoring_keys(4);
    let redeem_script = btc::RedeemScript::from_pubkeys(&pub_keys, 3).compressed(Network::Bitcoin);

    let tx = dummy_anchoring_tx(&redeem_script);
    let btc_signatures = make_signatures(&redeem_script, &tx, &[0], &priv_keys);

    let msg = MsgAnchoringSignature::new_with_signature(&PublicKey::zero(),
                                                        0,
                                                        tx,
                                                        1,
                                                        &btc_signatures[&0][0],
                                                        &Signature::zero());
    assert!(!msg.verify_content());
}
