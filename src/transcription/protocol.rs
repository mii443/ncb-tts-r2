use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

pub const PROTOCOL_VERSION: u8 = 2;
pub const AUDIO_KIND: u8 = 1;
pub const AUDIO_HEADER_SIZE: usize = 18;
pub const MAX_PCM_BYTES: usize = 256 * 1024;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("stream_id 0 is reserved")]
    ReservedStreamId,
    #[error("PCM payload must contain whole s16 samples")]
    InvalidPcm,
    #[error("PCM payload is too large")]
    PcmTooLarge,
    #[cfg(test)]
    #[error("audio frame is shorter than its header")]
    ShortFrame,
    #[cfg(test)]
    #[error("unsupported protocol version {0}")]
    UnsupportedVersion(u8),
    #[cfg(test)]
    #[error("unsupported binary frame kind {0}")]
    UnsupportedKind(u8),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioFrame {
    pub stream_id: u32,
    pub sequence: u32,
    pub captured_at_ms: u64,
    pub pcm: Vec<u8>,
}

impl AudioFrame {
    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        validate_pcm(self.stream_id, &self.pcm)?;
        let mut out = Vec::with_capacity(AUDIO_HEADER_SIZE + self.pcm.len());
        out.push(PROTOCOL_VERSION);
        out.push(AUDIO_KIND);
        out.extend_from_slice(&self.stream_id.to_be_bytes());
        out.extend_from_slice(&self.sequence.to_be_bytes());
        out.extend_from_slice(&self.captured_at_ms.to_be_bytes());
        out.extend_from_slice(&self.pcm);
        Ok(out)
    }

    #[cfg(test)]
    pub fn decode(input: &[u8]) -> Result<Self, ProtocolError> {
        if input.len() < AUDIO_HEADER_SIZE {
            return Err(ProtocolError::ShortFrame);
        }
        if input[0] != PROTOCOL_VERSION {
            return Err(ProtocolError::UnsupportedVersion(input[0]));
        }
        if input[1] != AUDIO_KIND {
            return Err(ProtocolError::UnsupportedKind(input[1]));
        }
        let stream_id = u32::from_be_bytes(input[2..6].try_into().expect("fixed slice"));
        let sequence = u32::from_be_bytes(input[6..10].try_into().expect("fixed slice"));
        let captured_at_ms = u64::from_be_bytes(input[10..18].try_into().expect("fixed slice"));
        let pcm = input[AUDIO_HEADER_SIZE..].to_vec();
        validate_pcm(stream_id, &pcm)?;
        Ok(Self {
            stream_id,
            sequence,
            captured_at_ms,
            pcm,
        })
    }
}

fn validate_pcm(stream_id: u32, pcm: &[u8]) -> Result<(), ProtocolError> {
    if stream_id == 0 {
        return Err(ProtocolError::ReservedStreamId);
    }
    if pcm.is_empty() || !pcm.len().is_multiple_of(2) {
        return Err(ProtocolError::InvalidPcm);
    }
    if pcm.len() > MAX_PCM_BYTES {
        return Err(ProtocolError::PcmTooLarge);
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize)]
pub struct Hello<'a> {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub protocol: u8,
    pub client: &'static str,
    pub format: &'static str,
    pub sr: u32,
    pub channels: u8,
    pub auth: &'a str,
    pub epoch: &'a str,
}

impl<'a> Hello<'a> {
    pub fn new(auth: &'a str, epoch: &'a str) -> Self {
        Self {
            kind: "hello",
            protocol: PROTOCOL_VERSION,
            client: "rstt",
            format: "pcm_s16le",
            sr: 16_000,
            channels: 1,
            auth,
            epoch,
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct StreamOpen {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub stream_id: u32,
    pub source: &'static str,
    pub speaker_id: String,
    pub speaker: String,
    pub started_at_ms: u64,
    pub metadata: Map<String, Value>,
}

impl StreamOpen {
    pub fn new(
        stream_id: u32,
        speaker_id: impl Into<String>,
        speaker: impl Into<String>,
        started_at_ms: u64,
        metadata: Map<String, Value>,
    ) -> Self {
        Self {
            kind: "stream_open",
            stream_id,
            source: "discord",
            speaker_id: speaker_id.into(),
            speaker: speaker.into(),
            started_at_ms,
            metadata,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct StreamControl<'a> {
    #[serde(rename = "type")]
    pub kind: &'a str,
    pub stream_id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'a str>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ServerMessage {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub protocol: Option<u8>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(flatten)]
    pub fields: Map<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_contract_matches_python_header() {
        let frame = AudioFrame {
            stream_id: 17,
            sequence: 42,
            captured_at_ms: 1_788_100_002_510,
            pcm: vec![0, 0, 1, 0, 255, 255, 255, 127, 0, 128],
        };
        let encoded = frame.encode().unwrap();
        assert_eq!(encoded.len(), AUDIO_HEADER_SIZE + 10);
        assert_eq!(&encoded[..2], &[2, 1]);
        assert_eq!(
            hex(&encoded),
            "0201000000110000002a000001a05310c2ce00000100ffffff7f0080"
        );
        assert_eq!(AudioFrame::decode(&encoded).unwrap(), frame);
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn rejects_zero_stream_and_odd_pcm() {
        assert_eq!(
            AudioFrame {
                stream_id: 0,
                sequence: 0,
                captured_at_ms: 0,
                pcm: vec![0, 0],
            }
            .encode(),
            Err(ProtocolError::ReservedStreamId)
        );
        assert_eq!(
            AudioFrame {
                stream_id: 1,
                sequence: 0,
                captured_at_ms: 0,
                pcm: vec![0],
            }
            .encode(),
            Err(ProtocolError::InvalidPcm)
        );
    }

    #[test]
    fn hello_has_exact_negotiation_values() {
        let value = serde_json::to_value(Hello::new("secret", "epoch")).unwrap();
        assert_eq!(value["type"], "hello");
        assert_eq!(value["protocol"], 2);
        assert_eq!(value["format"], "pcm_s16le");
        assert_eq!(value["sr"], 16_000);
        assert_eq!(value["channels"], 1);
    }
}
