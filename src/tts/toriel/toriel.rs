use anyhow::{anyhow, Result};
use mp3lame_encoder::{Builder, DualPcm, FlushNoGap, MonoPcm};
use std::io::Cursor;
use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

static VOICE_TORIEL_MP3: &[u8] = include_bytes!("voice_toriel.mp3");

#[derive(Debug, Clone)]
pub struct DecodedAudio {
    pub sample_rate: u32,
    pub channels: u32,
    pub samples_left: Vec<i16>,
    pub samples_right: Vec<i16>,
}

pub fn decode_toriel() -> Result<DecodedAudio> {
    let cursor = Cursor::new(VOICE_TORIEL_MP3);
    let src = MediaSourceStream::new(Box::new(cursor), Default::default());

    let mut hint = Hint::new();
    hint.with_extension("mp3");

    let meta_opts: MetadataOptions = Default::default();
    let fmt_opts: FormatOptions = Default::default();

    let probed = symphonia::default::get_probe()
        .format(&hint, src, &fmt_opts, &meta_opts)
        .map_err(|e| anyhow!("Failed to probe: {}", e))?;

    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or(anyhow!("no supported audio tracks"))?;

    let dec_opts: DecoderOptions = Default::default();
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &dec_opts)
        .map_err(|e| anyhow!("Failed to create decoder: {}", e))?;

    let track_id = track.id;
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or(anyhow!("Unknown sample rate"))?;
    let channel_count = track
        .codec_params
        .channels
        .ok_or(anyhow!("Unknown channel count"))?
        .count();

    let mut samples_left = Vec::new();
    let mut samples_right = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(symphonia::core::errors::Error::IoError(_)) => break,
            Err(e) => return Err(anyhow!("Failed to read packet: {}", e)),
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => match decoded {
                AudioBufferRef::S16(buf) => {
                    for &sample in buf.chan(0) {
                        samples_left.push(sample);
                    }
                    if channel_count > 1 {
                        for &sample in buf.chan(1) {
                            samples_right.push(sample);
                        }
                    }
                }
                AudioBufferRef::F32(buf) => {
                    for &sample in buf.chan(0) {
                        samples_left.push((sample * 32767.0) as i16);
                    }
                    if channel_count > 1 {
                        for &sample in buf.chan(1) {
                            samples_right.push((sample * 32767.0) as i16);
                        }
                    }
                }
                _ => return Err(anyhow!("Unsupported sample format (U8, S24, etc.)")),
            },
            Err(e) => return Err(anyhow!("Failed to decode: {}", e)),
        }
    }

    Ok(DecodedAudio {
        sample_rate,
        channels: channel_count as u32,
        samples_left,
        samples_right,
    })
}

pub fn encode_looped_mp3(
    audio: &DecodedAudio,
    repeat_count: usize,
    overlap_samples: usize,
) -> Result<Vec<u8>> {
    // オーバーラップを考慮してサンプルを結合
    let mut combined_left = Vec::new();
    let mut combined_right = Vec::new();

    for i in 0..repeat_count {
        if i == 0 {
            // 最初のループは全サンプルを追加
            combined_left.extend_from_slice(&audio.samples_left);
            if audio.channels > 1 {
                combined_right.extend_from_slice(&audio.samples_right);
            }
        } else {
            // 2回目以降はオーバーラップ部分を加算
            let overlap_start = combined_left.len() - overlap_samples.min(audio.samples_left.len());
            let actual_overlap = combined_left.len() - overlap_start;

            // オーバーラップ部分を加算
            for j in 0..actual_overlap {
                let old_val = combined_left[overlap_start + j] as i32;
                let new_val = audio.samples_left[j] as i32;
                combined_left[overlap_start + j] = (old_val + new_val).clamp(-32768, 32767) as i16;
            }
            // オーバーラップ以降の部分を追加
            combined_left.extend_from_slice(&audio.samples_left[actual_overlap..]);

            if audio.channels > 1 {
                for j in 0..actual_overlap {
                    let old_val = combined_right[overlap_start + j] as i32;
                    let new_val = audio.samples_right[j] as i32;
                    combined_right[overlap_start + j] =
                        (old_val + new_val).clamp(-32768, 32767) as i16;
                }
                combined_right.extend_from_slice(&audio.samples_right[actual_overlap..]);
            }
        }
    }

    let mut mp3_builder = Builder::new().expect("Create MP3 builder");
    mp3_builder
        .set_sample_rate(audio.sample_rate)
        .map_err(|e| anyhow!("Failed to set sample rate: {}", e))?;
    mp3_builder
        .set_num_channels(audio.channels as u8)
        .map_err(|e| anyhow!("Failed to set num channels: {}", e))?;
    mp3_builder
        .set_quality(mp3lame_encoder::Quality::Best)
        .map_err(|e| anyhow!("Failed to set quality: {}", e))?;
    mp3_builder
        .set_brate(mp3lame_encoder::Bitrate::Kbps192)
        .map_err(|e| anyhow!("Failed to set bitrate: {}", e))?;

    let mut encoder = mp3_builder.build().expect("Build MP3 encoder");
    let mut output_buffer: Vec<u8> = Vec::new();

    let mut mp3_buffer = vec![std::mem::MaybeUninit::uninit(); 8192];

    // 結合されたサンプルをチャンク単位でエンコード
    const CHUNK_SIZE: usize = 1152; // MP3フレームサイズ
    let total_samples = combined_left.len();

    for chunk_start in (0..total_samples).step_by(CHUNK_SIZE) {
        let chunk_end = (chunk_start + CHUNK_SIZE).min(total_samples);

        if audio.channels == 1 {
            let mono = MonoPcm(&combined_left[chunk_start..chunk_end]);
            let encoded_size = encoder
                .encode(mono, &mut mp3_buffer)
                .map_err(|e| anyhow!("Failed to encode: {}", e))?;
            unsafe {
                let slice =
                    std::slice::from_raw_parts(mp3_buffer.as_ptr() as *const u8, encoded_size);
                output_buffer.extend_from_slice(slice);
            }
        } else {
            let dual = DualPcm {
                left: &combined_left[chunk_start..chunk_end],
                right: &combined_right[chunk_start..chunk_end],
            };
            let encoded_size = encoder
                .encode(dual, &mut mp3_buffer)
                .map_err(|e| anyhow!("Failed to encode: {}", e))?;
            unsafe {
                let slice =
                    std::slice::from_raw_parts(mp3_buffer.as_ptr() as *const u8, encoded_size);
                output_buffer.extend_from_slice(slice);
            }
        }
    }

    let flushed_size = encoder
        .flush::<FlushNoGap>(&mut mp3_buffer)
        .map_err(|e| anyhow!("Failed to flush encoder: {}", e))?;
    unsafe {
        let slice = std::slice::from_raw_parts(mp3_buffer.as_ptr() as *const u8, flushed_size);
        output_buffer.extend_from_slice(slice);
    }

    Ok(output_buffer)
}

#[derive(Debug, Clone)]
pub struct TorielTTS {
    audio: DecodedAudio,
}

impl TorielTTS {
    pub fn new() -> Self {
        let decoded_audio = decode_toriel().expect("Failed to decode embedded MP3");
        Self {
            audio: decoded_audio,
        }
    }

    pub fn synthesize(&self, text: &str) -> Result<Vec<u8>> {
        let text_length = text.chars().count();
        let overlap_samples = (self.audio.sample_rate as f32 * 0.06) as usize;
        let mp3_data = encode_looped_mp3(&self.audio, text_length, overlap_samples)?;
        Ok(mp3_data)
    }
}
