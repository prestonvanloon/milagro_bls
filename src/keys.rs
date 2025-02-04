extern crate amcl;
extern crate rand;
extern crate zeroize;

use self::zeroize::Zeroize;
use super::amcl_utils::{self, BigNum, GroupG1, CURVE_ORDER, MOD_BYTE_SIZE};
use super::errors::DecodeError;
use super::g1::G1Point;
use super::rng::get_seeded_rng;
use rand::Rng;
#[cfg(feature = "std")]
use std::fmt;

#[derive(Clone)]
/// A BLS secret key.
pub struct SecretKey {
    pub x: BigNum,
}

impl SecretKey {
    /// Generate a new SecretKey using an Rng to seed the `amcl::rand::RAND` PRNG.
    pub fn random<R: Rng + ?Sized>(rng: &mut R) -> Self {
        let mut r = get_seeded_rng(rng, 256);
        let x = BigNum::randomnum(&BigNum::new_ints(&CURVE_ORDER), &mut r);
        SecretKey { x }
    }

    /// Instantiate a SecretKey from existing bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<SecretKey, DecodeError> {
        if bytes.len() != MOD_BYTE_SIZE {
            return Err(DecodeError::IncorrectSize);
        }
        Ok(SecretKey {
            x: BigNum::frombytes(bytes),
        })
    }

    /// Export the SecretKey to bytes.
    pub fn as_bytes(&self) -> Vec<u8> {
        let mut temp = BigNum::new_copy(&self.x);
        let mut bytes: [u8; MOD_BYTE_SIZE] = [0; MOD_BYTE_SIZE];
        temp.tobytes(&mut bytes);
        bytes.to_vec()
    }
}

#[cfg(feature = "std")]
impl fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut temp = BigNum::new();
        temp.copy(&self.x);
        write!(f, "{}", temp.tostring())
    }
}

impl PartialEq for SecretKey {
    fn eq(&self, other: &SecretKey) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl Eq for SecretKey {}

impl Drop for SecretKey {
    fn drop(&mut self) {
        self.x.w.zeroize();
    }
}

/// A BLS public key.
#[derive(Clone, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(Debug))]
pub struct PublicKey {
    pub point: G1Point,
}

impl PublicKey {
    /// Instantiate a PublicKey from some SecretKey.
    pub fn from_secret_key(sk: &SecretKey) -> Self {
        PublicKey {
            point: {
                #[cfg(feature = "std")]
                {
                    G1Point::from_raw(amcl_utils::GENERATORG1.mul_secret_key(&sk.x))
                }
                #[cfg(not(feature = "std"))]
                {
                    G1Point::from_raw(amcl_utils::GroupG1::generator().mul_secret_key(&sk.x))
                }
            },
        }
    }

    /// Instantiate a PublicKey from some GroupG1 point.
    pub fn new_from_raw(pt: &GroupG1) -> Self {
        PublicKey {
            point: G1Point::from_raw(*pt),
        }
    }

    /// Instantiate a PublicKey from compressed bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<PublicKey, DecodeError> {
        let point = G1Point::from_bytes(bytes)?;
        Ok(Self { point })
    }

    /// Export the PublicKey to compressed bytes.
    pub fn as_bytes(&self) -> Vec<u8> {
        let mut clone = self.point.clone();
        clone.as_bytes()
    }

    /// Export the public key to uncompress (x, y) bytes
    pub fn as_uncompressed_bytes(&mut self) -> Vec<u8> {
        if self.point.is_infinity() {
            return vec![0; 96];
        }

        let mut result: Vec<u8> = vec![];
        let mut bytes = [0 as u8; 48];
        self.point.getx().tobytes(&mut bytes);
        result.extend_from_slice(&bytes);
        self.point.gety().tobytes(&mut bytes);
        result.extend_from_slice(&bytes);
        result
    }

    /// InstantiatePublicKey from uncompress (x, y) bytes
    pub fn from_uncompressed_bytes(bytes: &[u8]) -> Result<PublicKey, DecodeError> {
        if bytes.len() != 96 {
            return Err(DecodeError::IncorrectSize);
        }

        let mut nil = true;
        for byte in bytes {
            if *byte != 0 {
                nil = false;
                break;
            }
        }
        if nil {
            // Point is infinity
            return Ok(PublicKey::new_from_raw(&GroupG1::new()));
        }

        let x_big = BigNum::frombytes(&bytes[0..48]);
        let y_big = BigNum::frombytes(&bytes[48..]);
        let point = GroupG1::new_bigs(&x_big, &y_big);

        if point.is_infinity() {
            return Err(DecodeError::BadPoint);
        }

        Ok(PublicKey::new_from_raw(&point))
    }
}

/// A helper which stores a BLS public and private key pair.
#[derive(Clone, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(Debug))]
pub struct Keypair {
    pub sk: SecretKey,
    pub pk: PublicKey,
}

impl Keypair {
    /// Instantiate a Keypair using SecretKey::random().
    pub fn random<R: Rng + ?Sized>(rng: &mut R) -> Self {
        let sk = SecretKey::random(rng);
        let pk = PublicKey::from_secret_key(&sk);
        Keypair { sk, pk }
    }
}

#[cfg(test)]
mod tests {
    extern crate hex;
    extern crate rand;
    extern crate yaml_rust;

    use self::yaml_rust::yaml;
    use super::super::amcl_utils::compress_g1;
    use super::super::signature::Signature;
    use super::*;
    use std::{fs::File, io::prelude::*, path::PathBuf};

    #[test]
    fn test_secret_key_serialization_isomorphism() {
        let sk_bytes = vec![
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 78, 252, 122, 126, 32, 0, 75, 89, 252,
            31, 42, 130, 254, 88, 6, 90, 138, 202, 135, 194, 233, 117, 181, 75, 96, 238, 79, 100,
            237, 59, 140, 111,
        ];
        let sk = SecretKey::from_bytes(&sk_bytes).unwrap();
        let decoded_sk = sk.as_bytes();
        assert_eq!(decoded_sk, sk_bytes);
    }

    #[test]
    fn test_public_key_serialization_isomorphism() {
        for _ in 0..30 {
            let sk = SecretKey::random(&mut rand::thread_rng());
            let pk = PublicKey::from_secret_key(&sk);
            let decoded_pk = pk.as_bytes();
            let encoded_pk = PublicKey::from_bytes(&decoded_pk).unwrap();
            let re_recoded_pk = encoded_pk.as_bytes();
            assert_eq!(decoded_pk, re_recoded_pk);
        }
    }

    #[test]
    fn test_public_key_uncompressed_serialization_isomorphism() {
        for _ in 0..30 {
            let sk = SecretKey::random(&mut rand::thread_rng());
            let mut pk = PublicKey::from_secret_key(&sk);
            let decoded_pk = pk.as_uncompressed_bytes();
            let mut encoded_pk = PublicKey::from_uncompressed_bytes(&decoded_pk).unwrap();
            let re_recoded_pk = encoded_pk.as_uncompressed_bytes();
            assert_eq!(decoded_pk, re_recoded_pk);
        }
    }

    #[test]
    fn test_public_key_uncompressed_serialization_infinity() {
        let sk_bytes = vec![0; 48];
        let sk = SecretKey::from_bytes(&sk_bytes).unwrap();
        let mut pk = PublicKey::from_secret_key(&sk);
        let decoded_pk = pk.as_uncompressed_bytes();
        let recoded_pk = PublicKey::from_uncompressed_bytes(&decoded_pk).unwrap();
        assert_eq!(recoded_pk, pk);
        assert!(recoded_pk.point.is_infinity())
    }

    #[test]
    fn test_public_key_uncompressed_serialization_incorrect_size() {
        let bytes = vec![0; 1];
        assert_eq!(
            PublicKey::from_uncompressed_bytes(&bytes),
            Err(DecodeError::IncorrectSize)
        );

        let bytes = vec![0; 95];
        assert_eq!(
            PublicKey::from_uncompressed_bytes(&bytes),
            Err(DecodeError::IncorrectSize)
        );

        let bytes = vec![0; 97];
        assert_eq!(
            PublicKey::from_uncompressed_bytes(&bytes),
            Err(DecodeError::IncorrectSize)
        );

        let bytes = vec![];
        assert_eq!(
            PublicKey::from_uncompressed_bytes(&bytes),
            Err(DecodeError::IncorrectSize)
        );
    }

    #[test]
    fn test_public_key_uncompressed_serialization_bad_point() {
        // Point (1, 1) is not valid
        let mut bytes = vec![0; 96];
        bytes[47] = 1;
        bytes[95] = 1;
        assert_eq!(
            PublicKey::from_uncompressed_bytes(&bytes),
            Err(DecodeError::BadPoint)
        );
    }

    #[test]
    fn test_signature_verify_with_serialized_public_key() {
        let sk_bytes = vec![
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 78, 252, 122, 126, 32, 0, 75, 89, 252,
            31, 42, 130, 254, 88, 6, 90, 138, 202, 135, 194, 233, 117, 181, 75, 96, 238, 79, 100,
            237, 59, 140, 111,
        ];
        let sk = SecretKey::from_bytes(&sk_bytes).unwrap();
        let pk = PublicKey::from_secret_key(&sk);
        let domain = 42;

        let message = "cats".as_bytes();
        let signature = Signature::new(&message, domain, &sk);
        assert!(signature.verify(&message, domain, &pk));

        let pk_bytes = pk.as_bytes();
        let pk = PublicKey::from_bytes(&pk_bytes).unwrap();
        assert!(signature.verify(&message, domain, &pk));
    }

    #[test]
    fn test_random_secret_key_can_sign() {
        let sk = SecretKey::random(&mut rand::thread_rng());
        let pk = PublicKey::from_secret_key(&sk);
        let domain = 42;

        let message = "cats".as_bytes();
        let signature = Signature::new(&message, domain, &sk);
        assert!(signature.verify(&message, domain, &pk));
    }

    // Test vector from https://github.com/ethereum/eth2.0-tests/blob/master/bls/test_bls.yml
    // case03_private_to_public_key
    #[test]
    fn case03_private_to_public_key() {
        // Run tests from test_bls.yml
        let mut file = {
            let mut file_path_buf = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            file_path_buf.push("src/test_vectors/test_bls.yml");

            File::open(file_path_buf).unwrap()
        };
        let mut yaml_str = String::new();
        file.read_to_string(&mut yaml_str).unwrap();
        let docs = yaml::YamlLoader::load_from_str(&yaml_str).unwrap();
        let doc = &docs[0];

        // Select test case03
        let test_cases = doc["case03_private_to_public_key"].as_vec().unwrap();

        // Verify input against output for each pair
        for test_case in test_cases {
            // Convert input to rust formats
            let input = test_case["input"].as_str().unwrap();
            // Convert privateKey from yaml to SecretKey
            let privkey = input.trim_start_matches("0x");
            let mut privkey = hex::decode(privkey).unwrap();
            while privkey.len() < 48 {
                // Prepend until correct length
                privkey.insert(0, 0);
            }
            let sk = SecretKey::from_bytes(&privkey).unwrap();

            // Create public key from private key and compress
            let pk = PublicKey::from_secret_key(&sk);
            let pk = compress_g1(&mut pk.point.as_raw().clone());

            // Convert given output to rust PublicKey
            let output = test_case["output"].as_str().unwrap();
            let output = output.trim_start_matches("0x");
            let output = hex::decode(output).unwrap();

            assert_eq!(output, pk);
        }
    }
}
