// Symphonia
// Copyright (c) 2019-2022 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![warn(rust_2018_idioms)]
#![forbid(unsafe_code)]
// The following lints are allowed in all Symphonia crates. Please see clippy.toml for their
// justification.
#![allow(clippy::comparison_chain)]
#![allow(clippy::excessive_precision)]
#![allow(clippy::identity_op)]
#![allow(clippy::manual_range_contains)]

pub mod flac;
pub mod id3v1;
pub mod id3v2;
pub mod itunes;
pub mod riff;
pub mod vorbis;

// Hermes: metadata fields are independent of the size of the playable media stream.
pub(crate) const MAX_METADATA_BYTES: usize = 4 * 1024 * 1024;
pub(crate) fn check_metadata_len<B: symphonia_core::io::ReadBytes>(
    reader: &B,
    length: usize,
) -> symphonia_core::errors::Result<()> {
    if length > MAX_METADATA_BYTES
        || reader
            .remaining_len()
            .is_some_and(|left| length as u64 > left)
    {
        return symphonia_core::errors::decode_error(
            "metadata length exceeds its allocation or enclosing packet limit",
        );
    }
    Ok(())
}
