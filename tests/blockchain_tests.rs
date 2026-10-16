use oorush::blockchain::{ChainState, OutPoint, Transaction, TxInput, TxOutput};
use oorush::pqc::dilithium_precompile::generate_keypair;
use pqcrypto_traits::sign::{PublicKey as _, SecretKey as _};

fn setup() -> (ChainState, Vec<u8>, Vec<u8>, String, Vec<u8>) {
    let (validator_pk, validator_sk) = generate_keypair();
    let (sender_pk, sender_sk) = generate_keypair();
    let sender_address = oorush::wallet::address_from_public_key(sender_pk.as_bytes());
    let chain = ChainState::create_genesis(
        vec![TxOutput {
            address: sender_address.clone(),
            amount: 100,
        }],
        vec![validator_pk.as_bytes().to_vec()],
        1,
    )
    .unwrap();
    (
        chain,
        validator_sk.as_bytes().to_vec(),
        sender_sk.as_bytes().to_vec(),
        sender_address,
        sender_pk.as_bytes().to_vec(),
    )
}

fn payment(
    chain: &ChainState,
    secret: &[u8],
    public: Vec<u8>,
    to: String,
    amount: u64,
) -> Transaction {
    let genesis_txid = chain.blocks[0].transactions[0].txid().unwrap();
    let mut tx = Transaction::unsigned(
        vec![TxInput {
            previous_output: OutPoint {
                txid: genesis_txid,
                index: 0,
            },
        }],
        vec![
            TxOutput {
                address: to,
                amount,
            },
            TxOutput {
                address: oorush::wallet::address_from_public_key(public.as_slice()),
                amount: 100 - amount,
            },
        ],
        public,
    );
    tx.sign(secret).unwrap();
    tx
}

#[test]
fn signed_transaction_and_balance_flow() {
    let (mut chain, validator, sender_secret, sender, sender_public) = setup();
    let (recipient_public, _) = generate_keypair();
    let recipient = oorush::wallet::address_from_public_key(recipient_public.as_bytes());
    let tx = payment(&chain, &sender_secret, sender_public, recipient.clone(), 40);
    assert!(tx.verify_signature().unwrap());
    chain.submit_transaction(tx.clone()).unwrap();
    let block = chain.produce_block(vec![tx], &validator, 2).unwrap();
    chain.validate_and_append_block(block).unwrap();
    assert_eq!(chain.balance(&sender).unwrap(), 60);
    assert_eq!(chain.balance(&recipient).unwrap(), 40);
}

#[test]
fn rejects_double_spend_and_tampered_transaction() {
    let (mut chain, _validator, sender_secret, _sender, sender_public) = setup();
    let recipient_public = generate_keypair().0;
    let recipient = oorush::wallet::address_from_public_key(recipient_public.as_bytes());
    let tx = payment(&chain, &sender_secret, sender_public, recipient, 40);
    chain.submit_transaction(tx.clone()).unwrap();
    assert!(chain.submit_transaction(tx.clone()).is_err());
    let mut tampered = tx;
    tampered.outputs[0].amount = 41;
    assert!(!tampered.verify_signature().unwrap());
}

#[test]
fn rejects_unauthorized_validator_and_round_trips_state() {
    let (chain, _validator, _sender_secret, _sender, _sender_public) = setup();
    let (_, unauthorized_secret) = generate_keypair();
    let result = chain.produce_block(Vec::new(), unauthorized_secret.as_bytes(), 2);
    assert!(result.is_err());
    let restored = ChainState::deserialize(&chain.serialize().unwrap()).unwrap();
    assert_eq!(restored.blocks, chain.blocks);
    assert_eq!(restored.utxos.len(), chain.utxos.len());
}

#[test]
fn rejects_forged_state_and_duplicate_serialized_utxo() {
    let (chain, ..) = setup();
    let mut value: serde_json::Value = serde_json::from_slice(&chain.serialize().unwrap()).unwrap();
    let pair = value[1][0].clone();
    value[1].as_array_mut().unwrap().push(pair);
    assert!(ChainState::deserialize(&serde_json::to_vec(&value).unwrap()).is_err());

    let mut value: serde_json::Value = serde_json::from_slice(&chain.serialize().unwrap()).unwrap();
    value[1][0][1]["amount"] = serde_json::json!(101);
    assert!(ChainState::deserialize(&serde_json::to_vec(&value).unwrap()).is_err());
}

#[test]
fn rejects_money_creation_burning_bad_addresses_zero_outputs_and_timestamp_rollback() {
    let (mut chain, validator, sender_secret, _sender, sender_public) = setup();
    let bad = payment(
        &chain,
        &sender_secret,
        sender_public.clone(),
        "not-an-address".into(),
        40,
    );
    assert!(chain.submit_transaction(bad).is_err());
    let mut created = Transaction::unsigned(
        vec![TxInput {
            previous_output: OutPoint {
                txid: chain.blocks[0].transactions[0].txid().unwrap(),
                index: 0,
            },
        }],
        vec![TxOutput {
            address: "oorush1".to_owned() + &"0".repeat(64),
            amount: 101,
        }],
        sender_public.clone(),
    );
    created.sign(&sender_secret).unwrap();
    assert!(chain.submit_transaction(created).is_err());
    let mut burned = payment(
        &chain,
        &sender_secret,
        sender_public.clone(),
        "oorush1".to_owned() + &"1".repeat(64),
        40,
    );
    burned.outputs[1].amount = 0;
    assert!(chain.submit_transaction(burned).is_err());
    assert!(chain.produce_block(Vec::new(), &validator, 0).is_err());
}
