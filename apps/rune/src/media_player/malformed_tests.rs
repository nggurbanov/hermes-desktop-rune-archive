//! Real malformed media headers must fail before allocating their declared payloads.
use std::{
    io::{self, Cursor},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use symphonia::core::{
    errors::Error,
    formats::{FormatOptions, FormatReader},
    io::{BufReader, MediaSourceStream, ReadBytes},
    meta::{Limit, MetadataBuilder, MetadataOptions, MetadataReader},
};

struct EmptyCounted(Arc<AtomicUsize>);
impl ReadBytes for EmptyCounted {
    fn read_byte(&mut self) -> io::Result<u8> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(io::ErrorKind::UnexpectedEof.into())
    }
    fn read_double_bytes(&mut self) -> io::Result<[u8; 2]> {
        Err(io::ErrorKind::UnexpectedEof.into())
    }
    fn read_triple_bytes(&mut self) -> io::Result<[u8; 3]> {
        Err(io::ErrorKind::UnexpectedEof.into())
    }
    fn read_quad_bytes(&mut self) -> io::Result<[u8; 4]> {
        Err(io::ErrorKind::UnexpectedEof.into())
    }
    fn read_buf(&mut self, _: &mut [u8]) -> io::Result<usize> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(io::ErrorKind::UnexpectedEof.into())
    }
    fn read_buf_exact(&mut self, _: &mut [u8]) -> io::Result<()> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(io::ErrorKind::UnexpectedEof.into())
    }
    fn scan_bytes_aligned<'a>(
        &mut self,
        _: &[u8],
        _: usize,
        _: &'a mut [u8],
    ) -> io::Result<&'a mut [u8]> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(io::ErrorKind::UnexpectedEof.into())
    }
    fn ignore_bytes(&mut self, _: u64) -> io::Result<()> {
        Err(io::ErrorKind::UnexpectedEof.into())
    }
    fn pos(&self) -> u64 {
        0
    }
}
#[test]
fn unbounded_packet_lengths_are_rejected_before_the_backing_source_is_read() {
    let reads = Arc::new(AtomicUsize::new(0));
    let mut source = EmptyCounted(reads.clone());
    for exact in [false, true] {
        let result = if exact {
            source.read_boxed_slice_exact(16 * 1024 * 1024 + 1)
        } else {
            source.read_boxed_slice(16 * 1024 * 1024 + 1)
        };
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert_eq!(reads.load(Ordering::SeqCst), 0);
    }
    let mut finite = BufReader::new(b"abc");
    assert_eq!(
        finite.read_boxed_slice_exact(1_000_000).unwrap_err().kind(),
        io::ErrorKind::UnexpectedEof
    );
    assert_eq!(finite.pos(), 0);
    assert_eq!(&*finite.read_boxed_slice(1_000_000).unwrap(), b"abc");
}
fn resource_error<T>(result: Result<T, Error>) {
    assert!(
        matches!(result, Err(Error::DecodeError(_))),
        "expected an explicit metadata limit verdict"
    );
}
#[test]
fn actual_vorbis_comments_reject_length_count_and_aggregate_before_payload_allocation() {
    let mut bytes = vec![];
    bytes.extend(0u32.to_le_bytes());
    bytes.extend(1u32.to_le_bytes());
    bytes.extend((32u32 * 1024 * 1024).to_le_bytes());
    resource_error(symphonia_metadata::vorbis::read_comment_no_framing(
        &mut BufReader::new(&bytes),
        &mut MetadataBuilder::new(),
    ));
    bytes.truncate(4);
    bytes.extend(5000u32.to_le_bytes());
    resource_error(symphonia_metadata::vorbis::read_comment_no_framing(
        &mut BufReader::new(&bytes),
        &mut MetadataBuilder::new(),
    ));
    let comment = b"TITLE=Field notes";
    let mut valid = vec![];
    valid.extend(0u32.to_le_bytes());
    valid.extend(1u32.to_le_bytes());
    valid.extend((comment.len() as u32).to_le_bytes());
    valid.extend(comment);
    let mut builder = MetadataBuilder::new();
    symphonia_metadata::vorbis::read_comment_no_framing(&mut BufReader::new(&valid), &mut builder)
        .unwrap();
    assert_eq!(builder.metadata().tags().len(), 1);
    let chunk = vec![b'x'; 3 * 1024 * 1024];
    let mut aggregate = vec![];
    aggregate.extend(0u32.to_le_bytes());
    aggregate.extend(2u32.to_le_bytes());
    for _ in 0..2 {
        aggregate.extend((chunk.len() as u32).to_le_bytes());
        aggregate.extend(&chunk);
    }
    resource_error(symphonia_metadata::vorbis::read_comment_no_framing(
        &mut BufReader::new(&aggregate),
        &mut MetadataBuilder::new(),
    ));
}
#[test]
fn actual_flac_picture_fields_cannot_allocate_declared_external_sizes() {
    for field in 0..3 {
        let mut bytes = vec![];
        bytes.extend(3u32.to_be_bytes());
        bytes.extend((if field == 0 { 32u32 * 1024 * 1024 } else { 0 }).to_be_bytes());
        if field > 0 {
            bytes.extend((if field == 1 { 32u32 * 1024 * 1024 } else { 0 }).to_be_bytes());
        }
        if field > 1 {
            for _ in 0..4 {
                bytes.extend(0u32.to_be_bytes());
            }
            bytes.extend((32u32 * 1024 * 1024).to_be_bytes());
        }
        resource_error(symphonia_metadata::flac::read_picture_block(
            &mut BufReader::new(&bytes),
            &mut MetadataBuilder::new(),
        ));
    }
}
fn id3(body: &[u8]) -> Vec<u8> {
    let n = body.len() as u32;
    let mut v = b"ID3\x03\0\0".to_vec();
    v.extend([
        (n >> 21) as u8 & 127,
        (n >> 14) as u8 & 127,
        (n >> 7) as u8 & 127,
        n as u8 & 127,
    ]);
    v.extend(body);
    v
}
#[test]
fn actual_id3_reader_honors_requested_metadata_budget_and_rejects_oversized_frames() {
    let mut body = b"TIT2".to_vec();
    body.extend(17u32.to_be_bytes());
    body.extend([0, 0, 0]);
    body.extend(b"Field notes title");
    let bytes = id3(&body);
    let mut stream =
        MediaSourceStream::new(Box::new(Cursor::new(bytes.clone())), Default::default());
    let mut limited = symphonia_metadata::id3v2::Id3v2Reader::new(&MetadataOptions {
        limit_metadata_bytes: Limit::Maximum(8),
        ..Default::default()
    });
    resource_error(limited.read_all(&mut stream));
    let mut ordinary = symphonia_metadata::id3v2::Id3v2Reader::new(&Default::default());
    let mut stream = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
    assert_eq!(ordinary.read_all(&mut stream).unwrap().tags().len(), 1);
    let mut huge = b"TIT2".to_vec();
    huge.extend((32u32 * 1024 * 1024).to_be_bytes());
    huge.extend([0, 0]);
    let bytes = id3(&huge);
    resource_error(symphonia_metadata::id3v2::read_id3v2(
        &mut BufReader::new(&bytes),
        &mut MetadataBuilder::new(),
    ));
}
#[test]
fn actual_mp4_tables_reject_counts_that_cannot_fit_the_enclosing_atom() {
    let original = include_bytes!("../../assets/test-audio/tone.m4a");
    for kind in [b"stsz", b"stts", b"stsc", b"stco"] {
        let mut bytes = original.to_vec();
        let at = bytes
            .windows(4)
            .position(|x| x == kind)
            .expect("actual AAC fixture table");
        let offset = if kind == b"stsz" {
            bytes[at + 8..at + 12].copy_from_slice(&0u32.to_be_bytes());
            at + 12
        } else {
            at + 8
        };
        bytes[offset..offset + 4].copy_from_slice(&u32::MAX.to_be_bytes());
        let source = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
        resource_error(symphonia::default::formats::IsoMp4Reader::try_new(
            source,
            &FormatOptions::default(),
        ));
    }
}
#[test]
fn actual_wave_info_rejects_a_declared_metadata_allocation_outside_its_list() {
    let mut bytes = b"RIFF".to_vec();
    bytes.extend(52u32.to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend([1, 0, 1, 0]);
    bytes.extend(48000u32.to_le_bytes());
    bytes.extend(96000u32.to_le_bytes());
    bytes.extend([2, 0, 16, 0]);
    bytes.extend(b"LIST");
    bytes.extend(12u32.to_le_bytes());
    bytes.extend(b"INFOINAM");
    bytes.extend((32u32 * 1024 * 1024).to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(0u32.to_le_bytes());
    let source = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
    resource_error(symphonia::default::formats::WavReader::try_new(
        source,
        &FormatOptions::default(),
    ));
}

#[test]
fn actual_mp4_zero_timebase_and_short_metadata_atoms_return_errors_without_panicking() {
    let original = include_bytes!("../../assets/test-audio/tone.m4a");
    let mut cases = vec![];
    let mut bytes = original.to_vec();
    let at = bytes.windows(4).position(|x| x == b"mdhd").unwrap();
    let offset = if bytes[at + 4] == 0 { at + 16 } else { at + 24 };
    bytes[offset..offset + 4].copy_from_slice(&0u32.to_be_bytes());
    cases.push(bytes);
    for kind in [b"stsd", b"meta", b"mdhd"] {
        let mut bytes = original.to_vec();
        let Some(at) = bytes.windows(4).position(|x| x == kind) else {
            continue;
        };
        bytes[at - 4..at].copy_from_slice(&8u32.to_be_bytes());
        cases.push(bytes);
    }
    for bytes in cases {
        let result = std::panic::catch_unwind(move || {
            let source = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
            symphonia::default::formats::IsoMp4Reader::try_new(source, &FormatOptions::default())
                .is_err()
        });
        assert!(
            matches!(result, Ok(true)),
            "Malformed container must return an error, never unwind its owned worker"
        );
    }
}
