//! Shared fixtures for the vfs text-primitive test binaries. [VFS-TEXT]

use lspkit::vfs::PositionEncoding;

/// The three LSP position encodings, for table-driven permutations. [VFS-TEXT]
pub const ALL_ENCODINGS: [PositionEncoding; 3] = [
    PositionEncoding::Utf8,
    PositionEncoding::Utf16,
    PositionEncoding::Utf32,
];
