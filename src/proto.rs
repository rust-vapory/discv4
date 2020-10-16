use crate::{
    message::*,
    util::{keccak256, pk2id},
    PeerId,
};
use anyhow::anyhow;
use bytes::BytesMut;
use k256::ecdsa::{
    recoverable::{Id as RecoveryId, Signature as RecoverableSignature},
    signature::{DigestSigner, Signature as _},
    Signature, SigningKey,
};
use primitive_types::H256;
use rlp::Rlp;
use sha3::{Digest, Keccak256};
use std::{io, iter::once};
use tokio::codec::{Decoder, Encoder};

macro_rules! try_none {
    ( $ex:expr ) => {
        match $ex {
            Ok(val) => val,
            Err(e) => return Ok(Some(Err(anyhow::Error::new(e)))),
        }
    };
}

pub struct DPTCodec {
    secret_key: SigningKey,
}

pub enum DPTCodecMessage {
    Ping(PingMessage),
    Pong(PongMessage),
    FindNeighbours(FindNeighboursMessage),
    Neighbours(NeighboursMessage),
}

impl DPTCodec {
    pub const fn new(secret_key: SigningKey) -> Self {
        Self { secret_key }
    }
}

impl Decoder for DPTCodec {
    type Item = anyhow::Result<(DPTCodecMessage, PeerId, H256)>;
    type Error = io::Error;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if buf.len() < 98 {
            return Ok(None);
        }

        let hash = keccak256(&buf[32..]);
        let check_hash = H256::from_slice(&buf[0..32]);
        if check_hash != hash {
            return Ok(Some(Err(anyhow!(
                "Hash check failed: computed {}, prefix {}",
                hash,
                check_hash
            ))));
        }

        let rec_id = try_none!(RecoveryId::new(buf[96]));
        let rec_sig = try_none!(RecoverableSignature::new(
            &try_none!(Signature::from_bytes(&buf[32..96])),
            rec_id
        ));
        let public_key =
            try_none!(rec_sig.recover_verify_key_from_digest(Keccak256::new().chain(&buf[97..])));
        let remote_id = pk2id(&public_key);

        let typ = buf[97];
        let data = &buf[98..];

        let message = match typ {
            1 => DPTCodecMessage::Ping(try_none!(Rlp::new(data).as_val())),
            2 => DPTCodecMessage::Pong(try_none!(Rlp::new(data).as_val())),
            3 => DPTCodecMessage::FindNeighbours(try_none!(Rlp::new(data).as_val())),
            4 => DPTCodecMessage::Neighbours(try_none!(Rlp::new(data).as_val())),
            other => return Ok(Some(Err(anyhow!("Invalid message type: {}", other)))),
        };

        Ok(Some(Ok((message, remote_id, hash))))
    }
}

impl Encoder for DPTCodec {
    type Item = DPTCodecMessage;
    type Error = io::Error;

    fn encode(&mut self, msg: Self::Item, buf: &mut BytesMut) -> Result<(), Self::Error> {
        let mut typdata = match &msg {
            DPTCodecMessage::Ping(message) => once(1).chain(rlp::encode(message)).collect(),
            DPTCodecMessage::Pong(message) => once(2).chain(rlp::encode(message)).collect(),
            DPTCodecMessage::FindNeighbours(message) => {
                once(3).chain(rlp::encode(message)).collect()
            }
            DPTCodecMessage::Neighbours(message) => once(4).chain(rlp::encode(message)).collect(),
        };

        let signature: RecoverableSignature = self
            .secret_key
            .sign_digest(Keccak256::new().chain(&typdata));

        let mut hashdata = signature.as_bytes().to_vec();
        hashdata.append(&mut typdata);

        buf.extend_from_slice(Keccak256::digest(&hashdata).as_slice());
        buf.extend_from_slice(&hashdata);

        Ok(())
    }
}
