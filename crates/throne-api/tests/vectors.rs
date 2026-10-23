//! Checks the crate against the shared protocol test vectors.
//!
//! Every case must derive the same keys, produce the same hello, seal every
//! frame to its exact `wire` bytes, open it back, and reject every invalid
//! frame for the given direction and counter.

use serde_json::Value;
use throne_api::protocol::{
    self, derive_keys, Direction, FrameOpener, FrameSealer, CLIENT_HELLO_SIZE, FRAME_HEADER_SIZE,
    RANDOM_SIZE,
};
use throne_api::ApiKey;

const VECTORS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/throne-api/vectors/v1.json"
);

fn hex_bytes(value: &Value, field: &str) -> Vec<u8> {
    let text = value[field]
        .as_str()
        .unwrap_or_else(|| panic!("field {field} is not a string"));
    hex::decode(text).unwrap_or_else(|e| panic!("field {field} is not hex: {e}"))
}

fn direction(text: &str) -> Direction {
    match text {
        "c2s" => Direction::ClientToServer,
        "s2c" => Direction::ServerToClient,
        other => panic!("unknown direction {other}"),
    }
}

fn to_array<const N: usize>(bytes: &[u8]) -> [u8; N] {
    bytes
        .try_into()
        .expect("vector field has the expected size")
}

#[test]
fn shared_vectors() {
    let text = std::fs::read_to_string(VECTORS).expect("read vectors file");
    let doc: Value = serde_json::from_str(&text).expect("vectors are JSON");
    let cases = doc["cases"].as_array().expect("cases is an array");
    assert!(!cases.is_empty());

    let (mut frame_count, mut invalid_count) = (0usize, 0usize);

    for case in cases {
        let name = case["name"].as_str().expect("case name");
        let key_text = case["key"].as_str().expect("case key");
        let key = ApiKey::from_text(key_text).unwrap_or_else(|e| panic!("{name}: key: {e}"));

        let server_hello = hex_bytes(case, "server_hello");
        let server_random: [u8; RANDOM_SIZE] = to_array(&server_hello);
        let client_hello = hex_bytes(case, "client_hello");
        assert_eq!(
            client_hello.len(),
            CLIENT_HELLO_SIZE,
            "{name}: client hello size"
        );
        let client_random: [u8; RANDOM_SIZE] = to_array(&client_hello[1..]);

        assert_eq!(
            protocol::client_hello(&client_random).to_vec(),
            client_hello,
            "{name}: client hello"
        );

        let keys = derive_keys(&key, &server_random, &client_random);
        assert_eq!(keys.c2s.to_vec(), hex_bytes(case, "k_c2s"), "{name}: k_c2s");
        assert_eq!(keys.s2c.to_vec(), hex_bytes(case, "k_s2c"), "{name}: k_s2c");

        for frame in case["frames"].as_array().expect("frames") {
            frame_count += 1;
            let dir = direction(frame["dir"].as_str().expect("dir"));
            let counter = frame["counter"].as_u64().expect("counter");
            // Plaintexts are UTF-8 text, not hex.
            let plaintext = frame["plaintext"]
                .as_str()
                .expect("plaintext is a string")
                .as_bytes()
                .to_vec();
            let wire = hex_bytes(frame, "wire");
            let dir_key = match dir {
                Direction::ClientToServer => keys.c2s,
                Direction::ServerToClient => keys.s2c,
            };

            let mut sealer = FrameSealer::with_counter(dir, dir_key, counter);
            assert_eq!(
                sealer.seal(&plaintext).expect("seal"),
                wire,
                "{name}: seal {counter}"
            );

            let (header, body) = wire.split_at(FRAME_HEADER_SIZE);
            let header: [u8; FRAME_HEADER_SIZE] = to_array(header);
            let mut opener = FrameOpener::with_counter(dir, dir_key, counter);
            assert_eq!(
                opener.open(header, body).expect("open"),
                plaintext,
                "{name}: open {counter}"
            );
        }

        for entry in case["invalid"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
            invalid_count += 1;
            let entry_name = entry["name"].as_str().expect("invalid name");
            let dir = direction(entry["dir"].as_str().expect("dir"));
            let counter = entry["counter"].as_u64().expect("counter");
            let wire = hex_bytes(entry, "wire");
            let dir_key = match dir {
                Direction::ClientToServer => keys.c2s,
                Direction::ServerToClient => keys.s2c,
            };

            // A wire shorter than the header cannot even be framed, so it is
            // rejected before the opener sees it.
            if wire.len() < FRAME_HEADER_SIZE {
                continue;
            }
            let (header, body) = wire.split_at(FRAME_HEADER_SIZE);
            let header: [u8; FRAME_HEADER_SIZE] = to_array(header);
            let mut opener = FrameOpener::with_counter(dir, dir_key, counter);
            assert!(
                opener.open(header, body).is_err(),
                "{name}: invalid frame '{entry_name}' was accepted"
            );
        }
    }

    println!(
        "vectors: {} cases, {frame_count} frames, {invalid_count} invalid entries",
        cases.len()
    );
}
